#!/usr/bin/env python3
"""Semantic-check gate for the native builder (dv-build) on Slang's own unit tests.

Each TEST_CASE in Slang's tests/unittests/ast/*.cpp that compiles one
source text is a small design with a known verdict. The text is run
through dv-build and through uvmcheck (Slang); per test file it reports
how many both accept, both reject, only Slang rejects (a native gap) and
only dv-build rejects (a native false error). Cases that set compilation
options or use several trees are skipped.

    tools/gates/slang_unit_gate.py [--list=gap|false|hang] [Assertion Cover ...]
"""
import collections, concurrent.futures, glob, json, os, re, resource, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
UVMCHECK = os.environ.get('UVMCHECK', os.path.join(REPO, 'build', 'uvmcheck'))
DVBUILD = os.environ.get('DV_BUILD', os.path.join(REPO, 'target', 'release', 'dv-build'))


def _default_tests():
    for b in ('build', 'build-dev', 'build-model'):
        p = os.path.join(REPO, b, '_deps', 'slang-src', 'tests', 'unittests', 'ast')
        if os.path.isdir(p):
            return p
    return os.path.join(REPO, 'build', '_deps', 'slang-src', 'tests', 'unittests', 'ast')


TESTS = os.environ.get('SLANG_TESTS', _default_tests())

CASE = re.compile(r'TEST_CASE\("((?:[^"\\]|\\.)*)"')
TEXT = re.compile(r'fromText\(R"\((.*?)\)"', re.S)


def cases(path):
    src = open(path, encoding='utf-8', errors='replace').read()
    starts = [(m.start(), m.group(1)) for m in CASE.finditer(src)]
    for i, (at, name) in enumerate(starts):
        body = src[at:starts[i + 1][0] if i + 1 < len(starts) else len(src)]
        texts = TEXT.findall(body)
        if not texts or re.search(r'fromFile', body):
            continue
        is_2023 = bool(re.search(r'v1800_2023', body))
        # Allow multi-tree cases and pure LanguageVersion::v1800_2023 options; skip custom compiler flags.
        if re.search(r'Options|Bag\b', body) and not is_2023:
            continue
        if not any(re.search(r'\b(module|package|interface|program|class|checker|primitive|config)\b', t) for t in texts):
            continue
        yield name, texts, is_2023


def limit():
    try:
        resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))
    except (ValueError, OSError):
        pass


def verdicts(texts, is_2023):
    with tempfile.TemporaryDirectory() as tmp:
        files = []
        for idx, text in enumerate(texts):
            f = os.path.join(tmp, f't{idx}.sv')
            open(f, 'w').write(text)
            files.append(f)
        nat_flags = ['--separate-units'] if len(files) > 1 else []
        try:
            nat = subprocess.run([DVBUILD, '--quiet', *nat_flags, *files], capture_output=True, timeout=30, preexec_fn=limit).returncode != 0
        except subprocess.TimeoutExpired:
            nat = 'hang'
        out = os.path.join(tmp, 'r.json')
        sl_flags = ['--std', '1800-2023'] if is_2023 else []
        try:
            subprocess.run([UVMCHECK, '--no-config', '--no-cache', *sl_flags, '--json', out, *files], capture_output=True, timeout=30, preexec_fn=limit)
        except subprocess.TimeoutExpired:
            pass
        try:
            sl = json.load(open(out)).get('compileErrors', 0) > 0
        except (OSError, ValueError):
            sl = True
        return nat, sl


def main():
    sel = [a for a in sys.argv[1:] if not a.startswith('--')]
    show = next((a.split('=', 1)[1] for a in sys.argv[1:] if a.startswith('--list=')), '')
    max_false = next((int(a.split('=', 1)[1]) for a in sys.argv[1:] if a.startswith('--max-false=')), None)
    max_gaps = next((int(a.split('=', 1)[1]) for a in sys.argv[1:] if a.startswith('--max-gaps=')), None)
    files = sorted(glob.glob(os.path.join(TESTS, '*Tests.cpp')))
    if sel:
        files = [f for f in files if any(s in os.path.basename(f) for s in sel)]
    jobs = [(os.path.basename(f)[:-9], n, ts, v23) for f in files for n, ts, v23 in cases(f)]
    if not jobs:
        print(f'FAIL: no Slang unit test cases found under {TESTS}', file=sys.stderr)
        return 1
    workers = int(os.environ.get('GATE_JOBS', min(2, os.cpu_count() or 2)))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        rows = list(pool.map(lambda j: (j[0], j[1], *verdicts(j[2], j[3])), jobs))
    t = collections.defaultdict(collections.Counter)
    hangs = 0
    for area, name, nat, sl in rows:
        if nat == 'hang':
            hangs += 1
        t[area]['ok' if not nat and not sl else 'reject' if nat and sl else 'gap' if sl else 'false'] += 1
    print(f'{"area":<14} {"both ok":>8} {"both reject":>12} {"gap":>6} {"false":>6}')
    tot = collections.Counter()
    for area in sorted(t):
        c = t[area]
        tot.update(c)
        print(f'{area:<14} {c["ok"]:>8} {c["reject"]:>12} {c["gap"]:>6} {c["false"]:>6}')
    print(f'{"total":<14} {tot["ok"]:>8} {tot["reject"]:>12} {tot["gap"]:>6} {tot["false"]:>6}')
    for area, name, nat, sl in rows:
        kind = 'hang' if nat == 'hang' else 'gap' if sl and not nat else 'false' if nat and not sl else ''
        if kind and kind == show:
            print(kind.upper(), area, '|', name)
    if hangs:
        return 1
    if max_false is not None and tot['false'] > max_false:
        return 1
    if max_gaps is not None and tot['gap'] > max_gaps:
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())

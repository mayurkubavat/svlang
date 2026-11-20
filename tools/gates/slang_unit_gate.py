#!/usr/bin/env python3
"""Semantic-check gate for the native builder (dv-build) on Slang's own unit tests.

Each TEST_CASE in Slang's tests/unittests/ast/*.cpp that compiles one
source text is a small design with a known verdict. The text is run
through dv-build and through svlang-lint (Slang); per test file it reports
how many both accept, both reject, only Slang rejects (a native gap) and
only dv-build rejects (a native false error). Cases that set compilation
options or use several trees are skipped.

    tools/gates/slang_unit_gate.py [--list=gap|false|hang] [Assertion Cover ...]
"""
import collections, concurrent.futures, glob, json, os, re, resource, signal, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


def _default_bin(env_key, candidates):
    if env_key in os.environ:
        return os.environ[env_key]
    for rel in candidates:
        p = os.path.join(REPO, *rel)
        if os.path.isfile(p) and os.access(p, os.X_OK):
            return p
    return os.path.join(REPO, *candidates[0])


SVLANG_LINT = _default_bin('SVLANG_LINT', [('build', 'svlang-lint'), ('build-model', 'svlang-lint')])
DVBUILD = _default_bin('DV_BUILD', [('target', 'release', 'dv-build'), ('target', 'debug', 'dv-build')])


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


MAX_RSS_KB = int(os.environ.get('GATE_MAX_RSS_MB', '1536')) * 1024


def limit():
    try:
        resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))
    except (ValueError, OSError):
        pass


def _pgid_rss_kb(pgid):
    try:
        r = subprocess.run(['ps', '-o', 'rss=', '-g', str(pgid)], capture_output=True, text=True, timeout=2)
        return sum(int(x) for x in r.stdout.split() if x.isdigit())
    except Exception:
        return 0


def run_bounded(cmd, timeout=20):
    """Run cmd in its own process group, killing the group on timeout or >MAX_RSS_KB RSS."""
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        preexec_fn=limit if sys.platform != 'darwin' else None,
    )
    deadline = time.monotonic() + timeout
    try:
        while True:
            rc = proc.poll()
            if rc is not None:
                return rc
            if time.monotonic() >= deadline:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                proc.wait()
                return 'hang'
            if _pgid_rss_kb(proc.pid) > MAX_RSS_KB:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                proc.wait()
                return 'oom'
            time.sleep(0.1)
    finally:
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()


def verdicts(texts, is_2023):
    with tempfile.TemporaryDirectory() as tmp:
        files = []
        for idx, text in enumerate(texts):
            f = os.path.join(tmp, f't{idx}.sv')
            open(f, 'w').write(text)
            files.append(f)
        nat_flags = ['--separate-units'] if len(files) > 1 else []
        nat_rc = run_bounded([DVBUILD, '--quiet', *nat_flags, *files], timeout=20)
        nat = nat_rc if isinstance(nat_rc, str) else (nat_rc != 0)
        out = os.path.join(tmp, 'r.json')
        sl_flags = ['--std', '1800-2023'] if is_2023 else []
        # Only compiler diagnostics are checked here: skip all lint rules and CrashGuard forking.
        run_bounded(
            [SVLANG_LINT, '--no-config', '--no-cache', '--rule', 'SV-CMP-001', '--override', '-crash-guard', *sl_flags, '--json', out, *files],
            timeout=20,
        )
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
    default_jobs = 1 if sys.platform == 'darwin' else min(2, os.cpu_count() or 2)
    workers = int(os.environ.get('GATE_JOBS', default_jobs))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        rows = list(pool.map(lambda j: (j[0], j[1], *verdicts(j[2], j[3])), jobs))
    t = collections.defaultdict(collections.Counter)
    hangs = 0
    for area, name, nat, sl in rows:
        if nat in ('hang', 'oom'):
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
        kind = nat if nat in ('hang', 'oom') else 'gap' if sl and not nat else 'false' if nat and not sl else ''
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

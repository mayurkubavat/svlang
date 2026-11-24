#!/usr/bin/env python3
"""IEEE 1800 conformance gate for the native compiler (`svlang check`) against Slang.

Runs every sv-tests file through `svlang check` and through `svlang_compile_check` (Slang oracle) and
reports, per LRM clause (the sv-tests chapter):
  legal        tests that must compile: how many each front end accepts;
  should_fail  tests that must be rejected: how many each front end rejects.
A legal test that `svlang check` rejects is a native false error and fails the
gate; a should_fail test only Slang rejects is a known gap, listed with
--gaps. Runtime-only should_fail tests (`:type: simulation` alone) are
counted apart: no front end can reject them.

    cmake --preset oracles && cmake --build build --target svlang_compile_check
    cargo build --release -p svlang-cli
    UVM_SRC=uvm-core/src tools/gates/ieee_gate.py ~/sv-tests [--gaps] [chapter-8]
"""
import collections, concurrent.futures, json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SLANG_CHECK = os.environ.get('SLANG_COMPILE_CHECK', os.path.join(REPO, 'build', 'svlang_compile_check'))
SVLANG = os.environ.get('SVLANG', os.path.join(REPO, 'target', 'release', 'svlang'))

CLAUSES = {
    '5': 'Lexical conventions', '6': 'Data types', '7': 'Aggregate data types', '8': 'Classes',
    '9': 'Processes', '10': 'Assignment statements', '11': 'Operators and expressions',
    '12': 'Procedural programming statements', '13': 'Tasks and functions', '14': 'Clocking blocks',
    '15': 'Interprocess synchronization', '16': 'Assertions', '18': 'Constrained random value generation',
    '20': 'Utility system tasks and functions', '21': 'Input/output system tasks', '22': 'Compiler directives',
    '23': 'Modules and hierarchy', '24': 'Programs', '25': 'Interfaces', '26': 'Packages',
}


def header(f):
    h = open(f, errors='replace').read()
    tag = lambda k: (re.search(rf'^:{k}:(.*)$', h, re.M) or [None, ''])[1].strip()
    return h, tag


def args_for(root, f, h):
    a = []
    m = re.search(r'^:defines:(.*)$', h, re.M)
    if m:
        a += ['+define+' + d for d in m.group(1).split()]
    m = re.search(r'^:incdirs:(.*)$', h, re.M)
    if m:
        a += ['+incdir+' + os.path.join(root, d) for d in m.group(1).split()]
    a += ['+incdir+' + os.path.dirname(f)]
    if os.environ.get('UVM_SRC'):
        a += ['+incdir+' + os.environ['UVM_SRC']]
        # As the sv-tests runner does: UVM tests compile uvm_pkg first.
        if re.search(r'^:tags:.*\buvm\b', h, re.M) or 'uvm_pkg' in h:
            a += [os.path.join(os.environ['UVM_SRC'], 'uvm_pkg.sv')]
    return a + [f]


def slang_rejects(args):
    r = subprocess.run([SLANG_CHECK, *args], capture_output=True, text=True, errors='replace', timeout=120)
    return r.returncode != 0


def native_rejects(args):
    r = subprocess.run([SVLANG, 'check', '--quiet', *args], capture_output=True, text=True, errors='replace', timeout=120)
    return r.returncode != 0


def run(root, f):
    h, tag = header(f)
    rel = os.path.relpath(f, os.path.join(root, 'tests'))
    top = rel.split('/')[0]
    clause = top.split('-')[1] if top.startswith('chapter-') else top
    bad = ':should_fail' in h
    kind = 'should_fail' if bad else 'legal'
    if bad and tag('type') == 'simulation':
        kind = 'runtime'
    a = args_for(root, f, h)
    return rel, clause, kind, native_rejects(a), slang_rejects(a), tag('should_fail_because')


def main():
    raw = sys.argv[1:]
    argv = []
    gaps = False
    max_gaps_arg = int(os.environ['IEEE_MAX_GAPS']) if os.environ.get('IEEE_MAX_GAPS') else None
    i = 0
    while i < len(raw):
        x = raw[i]
        if x == '--gaps':
            gaps = True
        elif x.startswith('--max-gaps='):
            max_gaps_arg = int(x.split('=', 1)[1])
        elif x == '--max-gaps' and i + 1 < len(raw):
            i += 1
            max_gaps_arg = int(raw[i])
        elif not x.startswith('--'):
            argv.append(x)
        i += 1
    root, sel = argv[0], (argv[1] if len(argv) > 1 else '')
    import glob
    files = [f for f in sorted(glob.glob(os.path.join(root, 'tests', '**', '*.sv'), recursive=True)) if sel in f]
    workers = int(os.environ.get('GATE_JOBS', min(4, os.cpu_count() or 4)))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        rows = list(pool.map(lambda f: run(root, f), files))
    t = collections.defaultdict(collections.Counter)
    for rel, clause, kind, nat, sl, why in rows:
        c = t[clause]
        c[kind] += 1
        c[kind + '_native'] += (not nat) if kind == 'legal' else nat
        c[kind + '_slang'] += (not sl) if kind == 'legal' else sl
    key = lambda c: (0, int(c)) if c.isdigit() else (1, c)
    print(f'{"clause":<8} {"title":<36} {"legal ok (native/slang/all)":>28} {"rejected (native/slang/all)":>28}')
    tot = collections.Counter()
    for clause in sorted(t, key=key):
        c = t[clause]
        tot.update(c)
        print(f'{clause:<8} {CLAUSES.get(clause, ""):<36} '
              f'{c["legal_native"]:>9}/{c["legal_slang"]}/{c["legal"]:<12} '
              f'{c["should_fail_native"]:>9}/{c["should_fail_slang"]}/{c["should_fail"]:<12}')
    print(f'{"total":<45} {tot["legal_native"]:>9}/{tot["legal_slang"]}/{tot["legal"]:<12} '
          f'{tot["should_fail_native"]:>9}/{tot["should_fail_slang"]}/{tot["should_fail"]:<12}')
    print(f'runtime-only should_fail tests (no front end can reject): {tot["runtime"]}')
    both_reject_legal = [r for r in rows if r[2] == 'legal' and r[3] and r[4]]
    if both_reject_legal:
        print(f'preprocess-only / upstream-invalid legal tests (both Slang and native reject): {len(both_reject_legal)}')
    false_errors = [r for r in rows if r[2] == 'legal' and r[3] and not r[4]]
    for r in false_errors:
        print('NATIVE FALSE ERROR', r[0])
    gap_rows = [r for r in rows if r[2] == 'should_fail' and r[4] and not r[3]]
    print(f'should_fail gaps (only Slang rejects): {len(gap_rows)}')
    if gaps:
        for r in gap_rows:
            print('GAP', r[0], '|', r[5][:100])
    if max_gaps_arg is not None and len(gap_rows) > max_gaps_arg:
        print(f'FAIL: should_fail gaps {len(gap_rows)} exceed budget {max_gaps_arg}')
        return 1
    return 1 if false_errors else 0


if __name__ == '__main__':
    sys.exit(main())

#!/usr/bin/env python3
"""Specialization counts: svlang's syntax engine (`svlang lint --specs` first column, which also
folds in body-level heuristics) or, with --model, sv-semantic's (third column,
sv_semantic::spec_counts: exact per class) against
slang's, as svlang-lint's AST walk sees them (build/specdump, see
tools/oracles/specdump.cpp). A generic class's body is visited once per count, so
every rule that walks bodies depends on these numbers.

    tools/gates/spec_gate.py [--model] [--show=N] --repo UVM_SRC | --uvm UVM_SRC | --dir DIR
"""
import concurrent.futures, glob, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
def _default_specdump():
    return os.path.join(REPO, 'build', 'svlang_specdump')


SPECDUMP = os.environ.get('SPECDUMP', _default_specdump())
SVLANG = os.environ.get('SVLANG', os.path.join(REPO, 'target', 'release', 'svlang'))


COL = 'syntax'


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True, errors='replace').stdout.splitlines()


def counts(args):
    ref = {}
    for l in run([SPECDUMP, *args]):
        c, k = l.split(maxsplit=1)
        ref[k] = int(c)
    ours = {}
    for l in run([SVLANG, 'lint', '--specs', *args]):
        c, k, m = l.split()
        v = m if COL == 'model' else c
        ours[k] = int(v) if v.isdigit() else -1
    return ref, ours


def main():
    argv = sys.argv[1:]
    show = 0
    global COL
    if argv and argv[0] == '--model':
        COL = 'model'
        argv.pop(0)
    max_differ = None
    while argv and argv[0].startswith('--max-differ='):
        max_differ = int(argv.pop(0).split('=', 1)[1])
    if argv and argv[0].startswith('--show='):
        show = int(argv.pop(0)[7:])
    if argv[:1] == ['--repo']:
        files = sorted(glob.glob(os.path.join(REPO, 'testdata/**/*.sv*'), recursive=True)
                       + glob.glob(os.path.join(REPO, 'python/**/*.sv*'), recursive=True))
        cases = [(os.path.relpath(f, REPO), ['+incdir+' + argv[1], '+incdir+' + os.path.join(REPO, 'testdata/common'),
                                            '+incdir+' + os.path.dirname(f), f]) for f in files]
    elif argv[:1] == ['--dir']:
        files = sorted(glob.glob(os.path.join(argv[1], '**/*.sv'), recursive=True))
        cases = [(os.path.relpath(f, argv[1]), ['+incdir+' + os.path.dirname(f), f]) for f in files]
    elif argv[:1] == ['--uvm']:
        cases = [('uvm_pkg', ['+incdir+' + argv[1], os.path.join(argv[1], 'uvm_pkg.sv')])]
    else:
        sys.exit(__doc__)
    if not os.path.isfile(SPECDUMP):
        print(f'FAIL: specdump binary not found at {SPECDUMP}', file=sys.stderr)
        return 1
    workers = int(os.environ.get('GATE_JOBS', min(2, os.cpu_count() or 2)))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        results = list(pool.map(lambda c: (c[0], counts(c[1])), cases))
    classes = good = 0
    bad_cases = 0
    for label, (ref, ours) in results:
        diffs = [(k, ref.get(k, 0), ours.get(k, 0)) for k in sorted(set(ref) | set(ours)) if ref.get(k, 0) != ours.get(k, 0)]
        classes += len(set(ref) | set(ours))
        good += len(set(ref) | set(ours)) - len(diffs)
        if diffs:
            bad_cases += 1
            if show:
                print(f'DIFF {label}')
                for k, r, o in diffs[:show]:
                    print(f'  {k}: slang {r}, ours {o}')
    print(f'compiles={len(cases)} differ={bad_cases} classes={classes} identical={good}')
    if classes == 0:
        print('FAIL: no generic classes checked (check input path)', file=sys.stderr)
        return 1
    if max_differ is not None and bad_cases > max_differ:
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())

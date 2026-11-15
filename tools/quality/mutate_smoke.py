#!/usr/bin/env python3
"""Robustness smoke test: svlang must never crash or hang, whatever the text.

    tools/quality/mutate_smoke.py <seed dir>... [--mutants N] [--jobs N] [--seed N]

Every `.sv`/`.v` file under the seed directories (the sv-tests checkout, this
repository's testdata) is damaged in small ways (a span deleted, the file cut
short, two spans swapped, a span doubled, a character replaced by a piece of
punctuation or a keyword) and given to `svlang check` and `svlang netlist`.
Either may report errors; neither may panic, be killed by a signal or run past
the time limit. The damage is chosen by a seeded generator, so a run can be
repeated, and each failure is printed with the command that reproduces it and
saved under --keep.

Exit status 1 when anything crashed or hung. SVLANG picks the binary
(default target/release/svlang).
"""
import argparse
import concurrent.futures
import hashlib
import os
import random
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PIECES = ["(", ")", "[", "]", "{", "}", ";", ",", "#", "@", "'", '"', "`", "::", "begin", "end", "endmodule",
          "module", "class", "function", "generate", "case", "property", "`define X", "`ifdef X", "`endif",
          "/*", "*/", "//", "\\", "'{", "<=", "|->", "##", "\n", "\x00", "é"]


def mutate(text, rng):
    """One damaged copy of `text`."""
    n = len(text)
    if n < 4:
        return text + rng.choice(PIECES)
    a = rng.randrange(n)
    b = min(n, a + rng.choice([1, 1, 2, 5, 20, 200]))
    kind = rng.randrange(6)
    if kind == 0:  # delete a span
        return text[:a] + text[b:]
    if kind == 1:  # cut short
        return text[:a]
    if kind == 2:  # double a span
        return text[:b] + text[a:b] + text[b:]
    if kind == 3:  # swap two spans
        c = rng.randrange(n)
        d = min(n, c + (b - a))
        if c < a:
            a, b, c, d = c, d, a, b
        if b > c:
            return text[:a] + text[b:]
        return text[:a] + text[c:d] + text[b:c] + text[a:b] + text[d:]
    if kind == 4:  # replace a span with a piece
        return text[:a] + rng.choice(PIECES) + text[b:]
    return text[:a] + rng.choice(PIECES) + text[a:]  # insert a piece


def seeds(dirs):
    out = []
    for d in dirs:
        for directory, _, files in os.walk(d):
            for name in sorted(files):
                if name.endswith((".sv", ".v", ".svh")):
                    out.append(os.path.join(directory, name))
    return sorted(out)


def run_seed(job):
    path, binary, mutants, base_seed, timeout, keep = job
    with open(path, encoding="utf-8", errors="replace") as f:
        text = f.read()
    if len(text) > 200_000:
        return []
    rng = random.Random(f"{base_seed}:{os.path.basename(path)}")
    failures = []
    with tempfile.TemporaryDirectory(prefix="svlang-mutate-") as tmp:
        for k in range(mutants):
            damaged = mutate(text, rng)
            # A second round of damage on half of them.
            if rng.random() < 0.5:
                damaged = mutate(damaged, rng)
            case = os.path.join(tmp, f"m{k}.sv")
            with open(case, "w", encoding="utf-8") as f:
                f.write(damaged)
            for command in ("check", "netlist"):
                try:
                    p = subprocess.run([binary, command, case], capture_output=True, timeout=timeout)
                    code, err = p.returncode, p.stderr.decode(errors="replace")
                except subprocess.TimeoutExpired:
                    code, err = "timeout", ""
                if code in (0, 1, 2):
                    continue
                digest = hashlib.sha1(damaged.encode()).hexdigest()[:12]
                saved = os.path.join(keep, f"{digest}.sv")
                os.makedirs(keep, exist_ok=True)
                with open(saved, "w", encoding="utf-8") as f:
                    f.write(damaged)
                where = next((line for line in err.splitlines() if "panicked at" in line), "")
                detail = next((line for line in err.splitlines()[1:] if line.strip() and "panicked at" not in line), "")
                failures.append((command, code, where.strip(), detail.strip()[:160], saved, path))
                break
    return failures


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dirs", nargs="+", help="directories of seed files")
    ap.add_argument("--mutants", type=int, default=4, help="damaged copies per seed file")
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=30, help="seconds per run")
    ap.add_argument("--keep", default=os.path.join(tempfile.gettempdir(), "svlang-mutate-failures"),
                    help="where failing inputs are saved")
    opts = ap.parse_args()
    binary = os.environ.get("SVLANG", os.path.join(REPO, "target", "release", "svlang"))
    if not os.path.exists(binary):
        sys.exit(f"no svlang binary at {binary}: cargo build --release -p svlang-cli, or set SVLANG")
    files = seeds(opts.dirs)
    jobs = [(f, binary, opts.mutants, opts.seed, opts.timeout, opts.keep) for f in files]
    with concurrent.futures.ThreadPoolExecutor(opts.jobs) as pool:
        failures = [f for result in pool.map(run_seed, jobs) for f in result]
    by_place = {}
    for command, code, where, detail, saved, path in failures:
        by_place.setdefault((where or f"exit {code}", detail), []).append((command, saved, path))
    print(f"{len(files)} seed files, {len(files) * opts.mutants} damaged inputs, 2 commands each: "
          f"{len(failures)} failing inputs at {len(by_place)} places")
    for (where, detail), cases in sorted(by_place.items(), key=lambda kv: -len(kv[1])):
        command, saved, path = cases[0]
        print(f"\n{len(cases)}x  {where}\n    {detail}\n    reproduce: {binary} {command} {saved}\n    from: {path}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

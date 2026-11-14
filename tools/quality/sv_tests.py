#!/usr/bin/env python3
"""svlang on sv-tests, the public SystemVerilog conformance suite
(https://github.com/chipsalliance/sv-tests).

    tools/quality/sv_tests.py fetch  <dir>     clone the suite's tests at the pinned commit into <dir>
    tools/quality/sv_tests.py run    <dir>     run `svlang check` on every test, write <dir>/results.jsonl
    tools/quality/sv_tests.py report <dir>     print the report (docs/reference/sv-tests.md)

The suite is third-party code (ISC license) and stays outside this
repository; only this tool and the report are checked in. Only the tests
stored in the suite's `tests` directory are run. The suite also generates
tests from other projects with its own scripts; those are not run here.

How a test is judged. Each test is one file with a header that names the
clause it covers (`:tags:`), what it exercises (`:type:`, some of
preprocessing, parsing, elaboration, simulation) and, for a negative test,
why a tool must refuse it (`:should_fail_because:`). `svlang check` does
everything up to and including elaboration; a test of the preprocessor alone
is run with `svlang preprocess`, since its text need not be a legal design. So:

    pass           svlang accepts a positive test, or reports an error in a negative one
    false reject   svlang reports an error in a positive test
    false accept   svlang accepts a negative test
    crash, timeout the tool died or ran past the time limit
    not run        the test is about simulation only, or needs the UVM library and
                   UVM_HOME is not set

A negative test whose failure only shows when the design runs (a `$fatal`,
a failing assertion) cannot be told from the header from one a compiler
must refuse; both are counted as negative tests.

Environment: SVLANG picks the svlang binary (default target/release/svlang),
UVM_HOME a UVM library checkout for the UVM tests, SLANG a slang binary to
say which of svlang's failures the reference front end shares.
"""
import argparse
import collections
import concurrent.futures
import datetime
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SUITE = "https://github.com/chipsalliance/sv-tests"
COMMIT = "415ba5f934efea7d95be4954c37a8f3e5783fea6"
STATIC = {"preprocessing", "parsing", "elaboration"}
FAILURES = ("false reject", "false accept", "crash", "timeout")


def git(args, cwd):
    subprocess.run(["git"] + args, cwd=cwd, check=True, stdout=subprocess.DEVNULL)


def fetch(root):
    """A sparse, shallow clone: the tests directory only, no submodules, nothing run."""
    suite = root
    if not os.path.isdir(os.path.join(suite, ".git")):
        os.makedirs(suite, exist_ok=True)
        git(["init", "-q"], suite)
        git(["remote", "add", "origin", SUITE + ".git"], suite)
        git(["sparse-checkout", "set", "tests"], suite)
    git(["fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", COMMIT], suite)
    git(["checkout", "-q", "FETCH_HEAD"], suite)
    print(f"sv-tests at {COMMIT[:12]} in {suite}")


def metadata(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        text = f.read()
    meta = {}
    for key, value in re.findall(r"^:([a-z_]+):[ \t]*(.*)$", text, re.M):
        meta.setdefault(key, value.strip())
    return meta


def tests(root):
    base = os.path.join(root, "tests")
    found = []
    for directory, _, files in os.walk(base):
        for name in sorted(files):
            if name.endswith((".sv", ".v")):
                path = os.path.join(directory, name)
                meta = metadata(path)
                if "name" in meta:
                    found.append((os.path.relpath(path, base), path, meta))
    return sorted(found)


def run_one(job):
    rel, path, meta, binary, uvm, slang, timeout = job
    tags = meta.get("tags", "").split()
    types = set(meta.get("type", "simulation elaboration parsing").split())
    negative = "should_fail_because" in meta
    row = {"test": rel, "name": meta["name"], "tags": tags, "types": sorted(types), "negative": negative}
    is_uvm = "uvm" in tags or rel.startswith(("uvm/", "testbenches/"))
    if not types & STATIC:
        return dict(row, outcome="not run", why="simulation only")
    if is_uvm and not uvm:
        return dict(row, outcome="not run", why="needs the UVM library")
    args = []
    if is_uvm:
        args += [f"+incdir+{uvm}/src", f"{uvm}/src/uvm_pkg.sv"]
    for define in meta.get("defines", "").split():
        args.append(f"+define+{define}")
    args.append(path)
    if "top_module" in meta:
        args += ["-top", meta["top_module"]]

    def tool(cmd):
        try:
            p = subprocess.run(cmd, capture_output=True, text=True, errors="replace", timeout=timeout)
            return p.returncode, p.stdout + p.stderr
        except subprocess.TimeoutExpired:
            return None, ""

    # A test of the preprocessor alone is judged on the preprocessor alone: its text need
    # not be a legal design.
    command = "preprocess" if types == {"preprocessing"} else "check"
    code, out = tool([binary, command] + args)
    if code is None:
        outcome = "timeout"
    elif code not in (0, 1):
        outcome = "crash"
    elif (code == 1) == negative:
        outcome = "pass"
    else:
        outcome = "false accept" if negative else "false reject"
    row["outcome"] = outcome
    if outcome in ("false reject", "crash"):
        errors = [line for line in out.splitlines() if ": error: " in line or "panicked" in line]
        first = errors[0] if errors else (out.strip().splitlines() or [""])[-1]
        row["message"] = re.sub(r"^.*?: error: ", "", first)[:200]
    if outcome in FAILURES and slang:
        slang_code, _ = tool([slang, "--quiet"] + args)
        row["reference"] = "timeout" if slang_code is None else ("rejects" if slang_code else "accepts")
    return row


def run(root, opts):
    binary = os.environ.get("SVLANG", os.path.join(REPO, "target", "release", "svlang"))
    if not os.path.exists(binary):
        sys.exit(f"no svlang binary at {binary}: cargo build --release -p svlang-cli, or set SVLANG")
    uvm, slang = os.environ.get("UVM_HOME"), os.environ.get("SLANG")
    version = subprocess.run(["git", "rev-parse", "--short=12", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip()
    jobs = [(rel, path, meta, binary, uvm, slang, opts.timeout) for rel, path, meta in tests(root)]
    with concurrent.futures.ThreadPoolExecutor(opts.jobs) as pool:
        rows = list(pool.map(run_one, jobs))
    out = opts.output or os.path.join(root, "results.jsonl")
    with open(out, "w") as f:
        f.write(json.dumps({"svlang": version, "suite": COMMIT, "date": datetime.date.today().isoformat(),
                            "uvm": bool(uvm), "reference": bool(slang)}) + "\n")
        for row in rows:
            f.write(json.dumps(row) + "\n")
    counts = collections.Counter(r["outcome"] for r in rows)
    print(f"{len(rows)} tests: " + ", ".join(f"{n} {k}" for k, n in counts.most_common()) + f" -> {out}")


def chapter(row):
    """The LRM clause number a test sits under (its directory), or its directory name."""
    head = row["test"].split("/")[0]
    return head.replace("chapter-", "") if head.startswith("chapter-") else head


CLAUSES = {
    "5": "Lexical conventions", "6": "Data types", "7": "Aggregate data types", "8": "Classes",
    "9": "Processes", "10": "Assignment statements", "11": "Operators and expressions",
    "12": "Procedural programming statements", "13": "Tasks and functions", "14": "Clocking blocks",
    "15": "Interprocess synchronization", "16": "Assertions", "18": "Constrained random value generation",
    "20": "Utility system tasks and functions", "21": "Input/output system tasks", "22": "Compiler directives",
    "23": "Modules and hierarchy", "24": "Programs", "25": "Interfaces", "26": "Packages",
    "generic": "Suite's own generic tests", "uvm": "UVM library", "testbenches": "UVM testbenches",
    "sanity.sv": "Suite's sanity test",
}


def report(root, opts):
    path = opts.output or os.path.join(root, "results.jsonl")
    with open(path) as f:
        head, *rows = [json.loads(line) for line in f]
    ran = [r for r in rows if r["outcome"] != "not run"]
    counts = collections.Counter(r["outcome"] for r in rows)
    passed = counts["pass"]
    p = print
    p("# sv-tests conformance\n")
    p(f"svlang on the tests of [sv-tests]({SUITE}), the public SystemVerilog conformance suite")
    p(f"(`tools/quality/sv_tests.py`). svlang `{head['svlang']}`, {head['date']}, sv-tests `{head['suite'][:12]}`.")
    p("Regenerate after a fix; the suite is kept outside the repository.\n")
    p(f"{len(rows)} tests in the suite's `tests` directory, {len(ran)} run: **{passed} pass ({100 * passed / len(ran):.1f}%)**, "
      f"{counts['false reject']} positive tests wrongly rejected, {counts['false accept']} negative tests wrongly accepted, "
      f"{counts['crash']} crashes, {counts['timeout']} timeouts.")
    skipped = collections.Counter(r["why"] for r in rows if r["outcome"] == "not run")
    if skipped:
        p(f"{sum(skipped.values())} not run: " + ", ".join(f"{n} {why}" for why, n in skipped.most_common()) + ".")
    p("\n`svlang check` runs each test through preprocessing, parsing, the semantic checks and elaboration; a test")
    p("of the preprocessor alone (`:type: preprocessing`) is run with `svlang preprocess`. A positive test passes")
    p("when no error is reported, a negative test (`:should_fail_because:`) when one is. The suite also generates")
    p("tests from other projects with its own scripts; those are not part of this run.\n")

    p("## By clause\n")
    p("| clause | | tests run | pass | false reject | false accept | crash or timeout | pass rate |")
    p("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    groups = collections.defaultdict(list)
    for r in ran:
        groups[chapter(r)].append(r)
    order = sorted(groups, key=lambda c: (not c.isdigit(), int(c) if c.isdigit() else 0, c))
    for c in order + ["all"]:
        g = ran if c == "all" else groups[c]
        n = collections.Counter(r["outcome"] for r in g)
        label = "**all**" if c == "all" else c
        rate = 100 * n["pass"] / len(g)
        p(f"| {label} | {CLAUSES.get(c, '')} | {len(g)} | {n['pass']} | {n['false reject']} | {n['false accept']} | "
          f"{n['crash'] + n['timeout']} | {rate:.0f}% |" if rate in (0, 100) or rate < 99.5 else
          f"| {label} | {CLAUSES.get(c, '')} | {len(g)} | {n['pass']} | {n['false reject']} | {n['false accept']} | "
          f"{n['crash'] + n['timeout']} | {rate:.1f}% |")

    def reference(r):
        return {"accepts": "accepts", "rejects": "rejects", "timeout": "timeout"}.get(r.get("reference"), "")

    rejected = [r for r in ran if r["outcome"] in ("false reject", "crash", "timeout")]
    if rejected:
        p("\n## Positive tests svlang rejects\n")
        p("Legal SystemVerilog that svlang reports an error in, grouped by the first error." +
          (" *Reference* is what slang does with the same test." if head.get("reference") else "") + "\n")
        by_message = collections.defaultdict(list)
        for r in rejected:
            message = r.get("message", r["outcome"])
            by_message[re.sub(r"`[^`]*`|'[^']*'|\"[^\"]*\"", "…", message)].append(r)
        p("| tests | first error | clauses | reference rejects too | example |")
        p("| ---: | --- | --- | ---: | --- |")
        for message, g in sorted(by_message.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            clauses = sorted({t for r in g for t in r["tags"] if t[0].isdigit()})
            shared = sum(1 for r in g if r.get("reference") == "rejects")
            p(f"| {len(g)} | {message.replace('|', chr(92) + '|')} | {', '.join(clauses[:4])}{'…' if len(clauses) > 4 else ''} | "
              f"{shared if head.get('reference') else ''} | `{g[0]['test']}` |")
    accepted = [r for r in ran if r["outcome"] == "false accept"]
    if accepted:
        p("\n## Negative tests svlang accepts\n")
        p("Tests a tool must refuse that svlang reports no error in. Some fail only when the design runs, which a")
        p("front end cannot see; the reference column tells those apart where slang accepts them too.\n")
        p("| test | clause | reference |")
        p("| --- | --- | --- |")
        for r in accepted:
            clauses = [t for t in r["tags"] if t[0].isdigit()]
            p(f"| `{r['test']}` | {', '.join(clauses)} | {reference(r)} |")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["fetch", "run", "report"])
    ap.add_argument("dir", help="where the suite is kept (outside the repository)")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--timeout", type=int, default=60, help="seconds per test")
    ap.add_argument("-o", "--output", help="results file (default <dir>/results.jsonl)")
    opts = ap.parse_args()
    {"fetch": lambda: fetch(opts.dir), "run": lambda: run(opts.dir, opts), "report": lambda: report(opts.dir, opts)}[opts.command]()


if __name__ == "__main__":
    main()

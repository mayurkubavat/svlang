#!/usr/bin/env python3
"""Lowering speed: how long svlang takes, stage by stage, on the census designs.

    tools/quality/lowering_bench.py run    <dir>     time every top the census lowers
    tools/quality/lowering_bench.py report <dir>     print docs/benchmarks/lowering.md

`<dir>` is the census directory (tools/quality/census.py): its designs and its
last results.jsonl, which says which tops lower. Each of those is compiled
again, alone, several times in a row and never two at once:

    svlang stages    the time inside each stage (parse, elaborate, lower, Btor2),
                     peak memory and the size of the netlist
    svlang check     the wall-clock time of the front end as a command
    slang            the same input through the reference front end, wall clock

The median of the runs is kept. Run it on a quiet machine: the numbers are
times, and anything else running moves them.

Environment: SVLANG and SLANG as for census.py.
"""
import argparse
import collections
import datetime
import json
import os
import platform
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import census  # noqa: E402


def wall(cmd, timeout):
    t0 = time.perf_counter()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, ""
    return time.perf_counter() - t0, p.stdout


def cpu_model():
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or platform.machine()


def run(base, opts):
    tools = census.find_tools()
    results = os.path.join(base, "results.jsonl")
    lowered = collections.defaultdict(set)
    with open(results) as f:
        for line in f:
            r = json.loads(line)
            if r.get("outcome") == "lowered":
                lowered[r["design"]].add(r["module"])
    rev = subprocess.run(["git", "rev-parse", "HEAD"], cwd=census.REPO, capture_output=True, text=True).stdout.strip()
    out_path = opts.output or os.path.join(base, "bench.jsonl")
    with open(out_path + ".tmp", "w") as out:
        meta = {
            "svlang": rev,
            "date": datetime.date.today().isoformat(),
            "runs": opts.runs,
            "cpu": cpu_model(),
            "cores": os.cpu_count(),
        }
        if tools["slang"]:
            meta["reference"] = subprocess.run([tools["slang"], "--version"], capture_output=True, text=True).stdout.strip()
        out.write(json.dumps({"meta": meta}) + "\n")
        for entry in census.load_corpus():
            name = entry["name"]
            if opts.only and name not in opts.only:
                continue
            if not lowered[name] or not os.path.isdir(os.path.join(base, name)):
                continue
            design = census.Design(entry, base)
            t0 = time.time()
            n = 0
            for module, path in design.units():
                if module not in lowered[name]:
                    continue
                files, incdirs = design.closure(path)
                incdirs = incdirs + [os.path.join(design.dir, x) for x in entry.get("incdirs", [])]
                defines = entry.get("defines", [])
                params = entry.get("params", {}).get(module, [])
                args = [f"+incdir+{d}" for d in incdirs] + [f"+define+{x}" for x in defines]
                args += [x for p in params for x in ("-G", p)]
                lines = 0
                for fpath in files:
                    try:
                        with open(fpath, "rb") as fh:
                            lines += sum(1 for _ in fh)
                    except OSError:
                        pass
                stages, rss, sizes, checks, refs = collections.defaultdict(list), [], {}, [], []
                for _ in range(opts.runs):
                    _, text = wall([tools["svlang"], "stages", *args, "-top", module, *files], opts.timeout)
                    try:
                        got = json.loads(text)
                    except ValueError:
                        continue
                    for st, secs in got.get("seconds", {}).items():
                        stages[st].append(secs)
                    if "peak_rss_kb" in got:
                        rss.append(got["peak_rss_kb"])
                    sizes = {k: got[k] for k in ("netlist", "btor2") if k in got}
                    t, _ = wall([tools["svlang"], "check", *args, "-top", module, *files], opts.timeout)
                    if t is not None:
                        checks.append(t)
                    if tools["slang"]:
                        sl = [tools["slang"], *census.REFERENCE_FLAGS, "--top", module]
                        sl += [x for d in incdirs for x in ("-I", d)] + [x for d in defines for x in ("-D", d)]
                        sl += [x for p in params for x in ("-G", p)]
                        t, _ = wall([*sl, *files], opts.timeout)
                        if t is not None:
                            refs.append(t)
                if not stages:
                    continue
                rec = {
                    "design": name,
                    "module": module,
                    "file": design.rel(path),
                    "files": len(files),
                    "lines": lines,
                    "seconds": {st: round(statistics.median(v), 6) for st, v in stages.items()},
                    "peak_rss_kb": max(rss) if rss else 0,
                    **sizes,
                }
                if checks:
                    rec["check_wall"] = round(statistics.median(checks), 6)
                if refs:
                    rec["reference_wall"] = round(statistics.median(refs), 6)
                out.write(json.dumps(rec, sort_keys=True) + "\n")
                n += 1
            out.flush()
            print(f"{name:16} {n:5} tops in {time.time() - t0:5.0f}s", file=sys.stderr)
    os.replace(out_path + ".tmp", out_path)


STAGES = ["parse", "elaborate", "lower", "btor2"]


def report(base, opts):
    path = opts.output or os.path.join(base, "bench.jsonl")
    meta, recs = {}, []
    with open(path) as f:
        for line in f:
            r = json.loads(line)
            if "meta" in r:
                meta = r["meta"]
            else:
                recs.append(r)
    corpus = [d["name"] for d in census.load_corpus()]
    w = sys.stdout.write

    def total(rs, st):
        return sum(r["seconds"].get(st, 0.0) for r in rs)

    def all_stages(r):
        return sum(r["seconds"].get(st, 0.0) for st in STAGES)

    def ms(x):
        return f"{1000 * x:,.1f}"

    w("# Lowering speed\n\n")
    w(
        "How long svlang takes on the designs of the [front-end census](../reference/census.md), stage by\n"
        "stage (`tools/quality/lowering_bench.py`). Every module the census lowers is compiled again as a top,\n"
        f"alone, {meta.get('runs', '?')} times in a row; the median is kept. svlang `{meta.get('svlang', '?')[:12]}`, "
        f"{meta.get('date', '')}, release build, one process at a time (CPU: {meta.get('cpu', '?')}, "
        f"{meta.get('cores', '?')} threads)"
        + (f"; reference front end {meta['reference']}" if meta.get("reference") else "")
        + ".\nRegenerate on a quiet machine; the numbers are times.\n\n"
    )
    lines = sum(r["lines"] for r in recs)
    secs = sum(all_stages(r) for r in recs)
    w(
        f"{len(recs)} tops, {lines:,} source lines compiled (a file used by several tops is counted for each). "
        f"All four stages take **{secs:.1f} s in total**, {lines / max(secs, 1e-9):,.0f} lines a second; "
        f"the median top takes {ms(statistics.median(all_stages(r) for r in recs))} ms and the slowest "
        f"{max(all_stages(r) for r in recs):.2f} s.\n\n"
    )
    w("Where the time goes: ")
    w(", ".join(f"{st} {100 * total(recs, st) / max(secs, 1e-9):.0f}%" for st in STAGES) + ".\n\n")

    w("## By design\n\n")
    w("Seconds are the sum over the design's tops; memory is the highest any one top needed.\n\n")
    w("| design | tops | lines | parse | elaborate | lower | Btor2 | all | lines/s | peak memory | netlist values |\n")
    w("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n")
    for name in corpus + ["all"]:
        rs = recs if name == "all" else [r for r in recs if r["design"] == name]
        if not rs:
            continue
        ln = sum(r["lines"] for r in rs)
        s = sum(all_stages(r) for r in rs)
        label = "**all**" if name == "all" else name
        w(
            f"| {label} | {len(rs)} | {ln:,} | "
            + " | ".join(f"{total(rs, st):.2f}" for st in STAGES)
            + f" | {s:.2f} | {ln / max(s, 1e-9):,.0f} | {max(r['peak_rss_kb'] for r in rs) / 1024:,.0f} MB "
            f"| {sum(r.get('netlist', {}).get('values', 0) for r in rs):,} |\n"
        )
    w("\n")

    both = [r for r in recs if "check_wall" in r and "reference_wall" in r]
    if both:
        w("## Against the reference front end\n\n")
        w(
            "The front end alone, as a command and by the wall clock: `svlang check` against the reference on the\n"
            "same files and top. Both parse, elaborate and run their checks; neither lowers. A command's start-up\n"
            "is part of each time, which matters for the many tops that take a few milliseconds.\n\n"
        )
        w("| design | tops | svlang check | reference | reference / svlang |\n| --- | ---: | ---: | ---: | ---: |\n")
        for name in corpus + ["all"]:
            rs = both if name == "all" else [r for r in both if r["design"] == name]
            if not rs:
                continue
            a, b = sum(r["check_wall"] for r in rs), sum(r["reference_wall"] for r in rs)
            label = "**all**" if name == "all" else name
            w(f"| {label} | {len(rs)} | {a:.2f} s | {b:.2f} s | {b / max(a, 1e-9):.1f}x |\n")
        w("\n")

    w("## The slowest tops\n\n")
    w("| top | files | lines | parse | elaborate | lower | Btor2 | all | netlist values | peak memory |\n")
    w("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n")
    for r in sorted(recs, key=all_stages, reverse=True)[: opts.rows]:
        w(
            f"| `{r['design']}` `{r['module']}` | {r['files']} | {r['lines']:,} | "
            + " | ".join(ms(r["seconds"].get(st, 0.0)) for st in STAGES)
            + f" | {ms(all_stages(r))} ms | {r.get('netlist', {}).get('values', 0):,} | {r['peak_rss_kb'] / 1024:,.0f} MB |\n"
        )
    w("\nStage times in milliseconds.\n\n")

    w("## The most memory\n\n")
    w("| top | lines | netlist values | Btor2 lines | peak memory |\n| --- | ---: | ---: | ---: | ---: |\n")
    for r in sorted(recs, key=lambda r: r["peak_rss_kb"], reverse=True)[:10]:
        w(
            f"| `{r['design']}` `{r['module']}` | {r['lines']:,} | {r.get('netlist', {}).get('values', 0):,} "
            f"| {r.get('btor2', {}).get('lines', 0):,} | {r['peak_rss_kb'] / 1024:,.0f} MB |\n"
        )
    w("\n")

    w("## Method and limits\n\n")
    w(
        "- Only tops that lower are timed; a module the census reports as unsupported or rejected stops early\n"
        "  and would look fast.\n"
        "- `lower` includes the checks lowering repeats on its own input, and `Btor2` is the writer with its\n"
        "  default options. Stage times come from inside the process (`svlang stages`), so they leave out its\n"
        "  start-up; the comparison with the reference uses the wall clock for both.\n"
        "- One process at a time, files read from a warm cache. The census's own run is in parallel and under\n"
        "  load, so its times are not these.\n"
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("command", choices=["run", "report"])
    ap.add_argument("dir", help="the census directory")
    ap.add_argument("--only", action="append", default=[], help="one design (repeatable)")
    ap.add_argument("--runs", type=int, default=3, help="runs per top; the median is kept")
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--rows", type=int, default=15)
    ap.add_argument("-o", "--output", help="results file (default <dir>/bench.jsonl)")
    opts = ap.parse_args()
    base = os.path.abspath(opts.dir)
    if opts.command == "run":
        run(base, opts)
    else:
        report(base, opts)


if __name__ == "__main__":
    main()

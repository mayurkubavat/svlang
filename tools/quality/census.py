#!/usr/bin/env python3
"""Front-end census: lower every module of a set of open RTL designs and rank
what stops them.

    tools/quality/census.py fetch  <dir>     clone each design at its pinned commit into <dir>
    tools/quality/census.py run    <dir>     lower every module, write <dir>/results.jsonl
    tools/quality/census.py report <dir>     print the ranked report (docs/reference/census.md)

The designs are listed in census_corpus.json with their commit and license.
They are third-party code and stay outside this repository; only this tool,
the list and the report are checked in.

How a module is measured. Each module defined under a design's `roots` is
made the top of its own compilation: the file that defines it plus the files
that define what it names (modules, interfaces, packages), found by a text
scan, in dependency order. `dv-build --netlist --top <module>` then ends in
one of:

    lowered        a netlist came out
    unsupported    legal SystemVerilog outside the netlist subset; the diagnostic names it
    rejected       svlang reports an error and the reference front end (slang) accepts the same input
    invalid        both reject it: the module cannot stand alone as a top with its default
                   parameters, or the design needs files or defines the census does not have
    accepted       svlang lowers it and the reference rejects it
    crash          the tool died
    memout/timeout it ran past the memory or time limit

`unsupported`, `rejected`, `crash`, `memout` and `timeout` are svlang's to fix and are
ranked. `invalid` is the census's own limit and is counted separately; a module
that is `invalid` as a top is still measured where another top instantiates it
(the "in a design" columns).

Environment: DV_BUILD picks the dv-build binary (default target/release),
SLANG the reference binary (default: `slang` on PATH; without it `rejected`
and `invalid` cannot be told apart and are reported together as `error`).
"""
import argparse
import collections
import concurrent.futures
import datetime
import fnmatch
import glob
import json
import os
import re
import resource
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
CORPUS_JSON = os.path.join(HERE, "census_corpus.json")

SOURCE_EXT = (".sv", ".v")
HEADER_EXT = (".svh", ".vh", ".h", ".inc", ".svi", ".vi")
# Directories that hold testbenches, formal harnesses and tool glue, not design RTL.
SKIP_DIR = re.compile(
    r"(^|/)(dv|tb|tbs|test|tests|testing|testbench|testbenches|bench|sim|simulation|fpv|formal|verif|"
    r"verification|uvm|uvmt|lint|syn|synth|doc|docs|example|examples|scripts|util|sva|bhv|"
    r"\.git|\.github)(/|$)"
)

COMMENT_OR_STRING = re.compile(r'"(?:\\.|[^"\\\n])*"|//[^\n]*|/\*.*?\*/', re.S)
INCLUDE = re.compile(r'`include\s+"([^"]+)"')
DEFINITION = re.compile(
    r"^[ \t]*(module|macromodule|interface|package|program|primitive)[ \t]+"
    r"(?:(?:static|automatic)[ \t]+)?([A-Za-z_][A-Za-z0-9_$]*)",
    re.M,
)
END_MODULE = re.compile(r"\bendmodule\b")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_$]*")
DIAG = re.compile(r"^(.*?):(\d+):(\d+): (unsupported|error): (.*)$")
# The reference runs the way a simulator would be run on these designs: its VCS
# compatibility mode, and a default time scale for files that set none.
REFERENCE_FLAGS = ["--quiet", "--error-limit=5", "--compat=vcs", "--timescale=1ns/1ps"]
HIER_INSTANCE = re.compile(r"^\S+ instance .* : ([A-Za-z_][A-Za-z0-9_$]*)$")


def load_corpus():
    with open(CORPUS_JSON) as f:
        return json.load(f)["designs"]


def git(args, cwd):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


# ---------------------------------------------------------------- fetch


def fetch(out, only):
    os.makedirs(out, exist_ok=True)
    for d in load_corpus():
        if only and d["name"] not in only:
            continue
        dest = os.path.join(out, d["name"])
        if not os.path.isdir(os.path.join(dest, ".git")):
            os.makedirs(dest, exist_ok=True)
            git(["init", "-q"], dest)
            git(["remote", "add", "origin", d["repo"]], dest)
        head = subprocess.run(["git", "rev-parse", "--verify", "-q", "HEAD"], cwd=dest, capture_output=True, text=True)
        have = head.stdout.strip() if head.returncode == 0 else ""
        want = d.get("rev") or "HEAD"
        if not have or have != want and d.get("rev"):
            git(["fetch", "-q", "--depth", "1", "origin", want], dest)
            git(["checkout", "-q", "--detach", "FETCH_HEAD"], dest)
            have = git(["rev-parse", "HEAD"], dest)
        print(f"{d['name']:16} {have}")


# ---------------------------------------------------------------- scan


class SourceFile:
    __slots__ = ("path", "defs", "modules", "idents", "includes")

    def __init__(self, path):
        self.path = path
        try:
            with open(path, errors="replace") as f:
                text = f.read()
        except OSError:
            text = ""
        # Comments become spaces (lines keep their numbers); strings stay for `include.
        nocomment = COMMENT_OR_STRING.sub(
            lambda m: m.group(0) if m.group(0)[0] == '"' else re.sub(r"[^\n]", " ", m.group(0)), text
        )
        self.includes = INCLUDE.findall(nocomment)
        code = COMMENT_OR_STRING.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)
        self.defs = []  # (kind, name)
        self.modules = []  # (name, first line, last line)
        seen = set()
        for m in DEFINITION.finditer(code):
            kind, name = m.group(1), m.group(2)
            if kind == "interface" and name == "class":
                continue
            if name in seen:
                continue
            seen.add(name)
            self.defs.append((kind, name))
            if kind in ("module", "macromodule"):
                start = code.count("\n", 0, m.start()) + 1
                end_m = END_MODULE.search(code, m.end())
                end = code.count("\n", 0, end_m.start()) + 1 if end_m else code.count("\n") + 1
                self.modules.append((name, start, end))
        self.idents = set(IDENT.findall(code))


def expand(base, patterns):
    """Files under the glob patterns (relative to base); a directory means everything below it."""
    out = []
    for pat in patterns:
        for hit in sorted(glob.glob(os.path.join(base, pat), recursive=True)):
            if os.path.isdir(hit):
                for root, dirs, files in os.walk(hit):
                    dirs.sort()
                    for f in sorted(files):
                        out.append(os.path.join(root, f))
            else:
                out.append(hit)
    return out


class Design:
    """One corpus entry, scanned: which file defines what, and what each file needs."""

    def __init__(self, entry, base):
        self.entry = entry
        self.name = entry["name"]
        self.dir = os.path.join(base, self.name)
        self.base = base
        exclude = [re.compile(x) for x in entry.get("exclude", [])]

        def keep(path, root_dir):
            rel = os.path.relpath(path, self.dir)
            if any(x.search(rel) for x in exclude):
                return False
            # The skip list applies below the listed root, so a root may itself be named `sim`.
            inner = os.path.relpath(path, root_dir) if os.path.isdir(root_dir) else ""
            return not SKIP_DIR.search(os.path.dirname(inner))

        def collect(patterns, anchor):
            got = []
            for pat in patterns:
                for hit in sorted(glob.glob(os.path.join(anchor, pat), recursive=True)):
                    for p in expand(anchor, [os.path.relpath(hit, anchor)]):
                        if p.endswith(SOURCE_EXT) and keep(p, hit):
                            got.append(p)
            return list(dict.fromkeys(got))

        self.root_files = collect(entry["roots"], self.dir)
        # deps: searched for definitions, not measured. "../other/src" reaches another design.
        dep_files = [p for p in collect(entry.get("deps", []), self.dir) if p not in set(self.root_files)]
        self.files = {p: SourceFile(p) for p in self.root_files + dep_files}

        # Headers anywhere in the design (and in the designs deps reach), by basename.
        self.headers = collections.defaultdict(list)
        header_dirs = {self.dir}
        for pat in entry.get("deps", []):
            if pat.startswith("../"):
                header_dirs.add(os.path.join(base, pat.split("/")[1]))
        for hd in sorted(header_dirs):
            for root, dirs, files in os.walk(hd):
                dirs[:] = sorted(x for x in dirs if x != ".git")
                for f in sorted(files):
                    self.headers[f].append(os.path.join(root, f))
        self.header_cache = {}

        self.defs = collections.defaultdict(list)  # name -> [path]
        for p, sf in self.files.items():
            for _, name in sf.defs:
                self.defs[name].append(p)
        self.prefer = entry.get("prefer", [])

    def rel(self, path):
        return os.path.relpath(path, self.base)

    def resolve_include(self, name, from_file):
        """The file an `include names: beside the includer, else by path suffix anywhere in the design."""
        beside = os.path.normpath(os.path.join(os.path.dirname(from_file), name))
        if os.path.isfile(beside):
            return beside
        cands = [p for p in self.headers.get(os.path.basename(name), []) if p.endswith("/" + name.lstrip("./"))]
        if not cands:
            return None
        return self.pick(cands, from_file)

    def pick(self, cands, from_file):
        """One of several files that define the same name: a preferred path, else the nearest."""
        if len(cands) == 1:
            return cands[0]
        for pat in self.prefer:
            for c in cands:
                if fnmatch.fnmatch(os.path.relpath(c, self.dir), pat):
                    return c
        here = os.path.dirname(from_file).split(os.sep)

        def nearness(c):
            parts = os.path.dirname(c).split(os.sep)
            n = 0
            while n < min(len(parts), len(here)) and parts[n] == here[n]:
                n += 1
            return (-n, len(parts), c)

        return min(cands, key=nearness)

    def needs(self, path):
        """(identifiers the file and its includes use, include directories they need)."""
        if path in self.header_cache:
            return self.header_cache[path]
        self.header_cache[path] = (set(), set())  # include cycles end here
        sf = self.files.get(path) or SourceFile(path)
        idents, incdirs = set(sf.idents), set()
        for inc in sf.includes:
            target = self.resolve_include(inc, path)
            if not target:
                continue
            # The directory that makes `include "a/b.svh"` resolve.
            suffix = inc.lstrip("./")
            if target.endswith(suffix):
                incdirs.add(os.path.normpath(target[: -len(suffix)]) or "/")
            sub_idents, sub_dirs = self.needs(target)
            idents |= sub_idents
            incdirs |= sub_dirs
        self.header_cache[path] = (idents, incdirs)
        return idents, incdirs

    def closure(self, path):
        """Files to compile for `path`, dependencies first, and the include directories."""
        order, incdirs, state = [], set(), {}

        def visit(p):
            if p in state:
                return
            state[p] = 1
            idents, dirs = self.needs(p)
            incdirs.update(dirs)
            own = {name for _, name in self.files[p].defs} if p in self.files else set()
            for name in sorted(idents & self.defs.keys()):
                if name in own:
                    continue
                visit(self.pick(self.defs[name], p))
            order.append(p)

        sys.setrecursionlimit(10000)
        visit(path)
        return order, sorted(incdirs)

    def units(self):
        for p in self.root_files:
            for name, start, end in self.files[p].modules:
                yield name, p

    def owner(self, path, line):
        """The module whose source text holds this location, or the file's own name."""
        sf = self.files.get(path)
        if sf:
            for name, start, end in sf.modules:
                if start <= line <= end:
                    return name
        return os.path.basename(path)


# ---------------------------------------------------------------- run


def limit_child(mem_gb):
    def apply():
        os.nice(15)
        cap = int(mem_gb * (1 << 30))
        resource.setrlimit(resource.RLIMIT_AS, (cap, cap))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

    return apply


def run_tool(cmd, timeout, mem_gb):
    t0 = time.time()
    try:
        p = subprocess.run(
            cmd, capture_output=True, text=True, errors="replace", timeout=timeout, preexec_fn=limit_child(mem_gb)
        )
        return p.returncode, p.stdout, p.stderr, time.time() - t0
    except subprocess.TimeoutExpired:
        return None, "", "", time.time() - t0


def measure(design, module, path, tools, opts):
    files, incdirs = design.closure(path)
    args = [f"+incdir+{d}" for d in incdirs + [os.path.join(design.dir, x) for x in design.entry.get("incdirs", [])]]
    defines = design.entry.get("defines", []) + opts.define
    args += [f"+define+{x}" for x in defines]
    rec = {"design": design.name, "module": module, "file": design.rel(path), "files": len(files)}

    code, out, err, secs = run_tool(
        [tools["dv_build"], "--netlist", "--hier", *args, "--top", module, *files], opts.timeout, opts.mem
    )
    rec["seconds"] = round(secs, 2)
    diags, instantiated = [], set()
    # Diagnostics and the hierarchy come before the netlist dump, which can run to megabytes.
    for line in out.splitlines():
        if line.startswith("netlist "):
            break
        m = DIAG.match(line)
        if m:
            f, ln = os.path.abspath(m.group(1)), int(m.group(2))
            diags.append(
                {
                    "kind": m.group(4),
                    "message": m.group(5).replace(design.base + os.sep, ""),
                    "file": design.rel(f),
                    "line": ln,
                    "owner": design.owner(f, ln),
                }
            )
            continue
        m = HIER_INSTANCE.match(line)
        if m:
            instantiated.add(m.group(1))
    rec["diags"] = diags[:50]
    rec["diag_count"] = len(diags)
    rec["instantiates"] = sorted(instantiated - {module})

    if code is None:
        rec["outcome"] = "timeout"
    elif code == 0:
        rec["outcome"] = "lowered"
    elif code == 1 and diags:
        rec["outcome"] = "unsupported" if all(d["kind"] == "unsupported" for d in diags) else "error"
    elif "memory allocation of" in err:
        rec["outcome"] = "memout"
    else:
        rec["outcome"] = "crash"
        lines = [x.strip() for x in err.splitlines() if x.strip() and not x.startswith("note: ")]
        at = next((i for i, x in enumerate(lines) if "panicked at" in x), None)
        rec["crash"] = {
            "exit": code,
            "message": (lines[at + 1] if at is not None and at + 1 < len(lines) else lines[-1] if lines else ""),
            "where": re.sub(r"^.*panicked at ", "", lines[at]).rstrip(":") if at is not None else "",
        }

    # The reference front end says whether the input itself is good.
    if tools["slang"] and rec["outcome"] in ("error", "lowered"):
        sl = [tools["slang"], *REFERENCE_FLAGS, "--top", module]
        sl += [x for d in incdirs + [os.path.join(design.dir, x) for x in design.entry.get("incdirs", [])] for x in ("-I", d)]
        sl += [x for d in defines for x in ("-D", d)]
        scode, sout, serr, _ = run_tool([*sl, *files], opts.timeout, opts.mem)
        if scode is None:
            rec["reference"] = "timeout"
        elif scode == 0:
            rec["reference"] = "accepts"
        else:
            rec["reference"] = "rejects"
            first = next((x for x in (sout + serr).splitlines() if ": error: " in x), "")
            m = re.match(r"^(.*?):(\d+):(\d+): error: (.*)$", first)
            if m:
                rec["reference_error"] = {
                    "file": design.rel(os.path.abspath(m.group(1))),
                    "line": int(m.group(2)),
                    "message": m.group(4).replace(design.base + os.sep, ""),
                }
        if rec["outcome"] == "error":
            rec["outcome"] = {"accepts": "rejected", "rejects": "invalid"}.get(rec["reference"], "error")
        elif rec["reference"] == "rejects":
            rec["outcome"] = "accepted"
    return rec


def find_tools():
    dv_build = os.environ.get("DV_BUILD") or os.path.join(REPO, "target", "release", "dv-build")
    if not os.path.isfile(dv_build):
        sys.exit(f"error: {dv_build} not found; cargo build --release -p dv-build, or set DV_BUILD")
    slang = os.environ.get("SLANG") or shutil.which("slang")
    if not slang:
        print("note: no reference front end (set SLANG); rejected and invalid are reported as error", file=sys.stderr)
    return {"dv_build": dv_build, "slang": slang}


def run(base, only, opts):
    tools = find_tools()
    rev = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip()
    dirty = subprocess.run(["git", "status", "--porcelain", "crates"], cwd=REPO, capture_output=True, text=True).stdout
    out_path = opts.output or os.path.join(base, "results.jsonl")
    kept = []
    if only and os.path.isfile(out_path):
        with open(out_path) as f:
            kept = [l for l in f if '"design"' in l and json.loads(l)["design"] not in only]
    tmp = out_path + ".tmp"
    with open(tmp, "w") as out:
        meta = {"svlang": rev + ("+local" if dirty.strip() else ""), "date": datetime.date.today().isoformat()}
        meta["timeout"], meta["mem"] = opts.timeout, opts.mem
        if opts.define:
            meta["defines"] = opts.define
        if tools["slang"]:
            v = subprocess.run([tools["slang"], "--version"], capture_output=True, text=True).stdout.strip()
            meta["reference"] = v + " " + " ".join(REFERENCE_FLAGS[2:])
        out.write(json.dumps({"meta": meta}) + "\n")
        out.writelines(kept)
        for entry in load_corpus():
            if only and entry["name"] not in only:
                continue
            if not os.path.isdir(os.path.join(base, entry["name"])):
                print(f"{entry['name']}: not fetched, skipped", file=sys.stderr)
                continue
            t0 = time.time()
            design = Design(entry, base)
            units = list(design.units())
            tally = collections.Counter()
            with concurrent.futures.ThreadPoolExecutor(opts.jobs) as pool:
                for rec in pool.map(lambda u: measure(design, u[0], u[1], tools, opts), units):
                    tally[rec["outcome"]] += 1
                    out.write(json.dumps(rec, sort_keys=True) + "\n")
            out.flush()
            summary = ", ".join(f"{k} {v}" for k, v in sorted(tally.items()))
            print(f"{design.name:16} {len(units):5} modules in {time.time() - t0:5.0f}s: {summary}", file=sys.stderr)
    os.replace(tmp, out_path)


# ---------------------------------------------------------------- report

# What a diagnostic is about, with the names and numbers taken out, so the same
# construct counts once however many signals it touches.
QUOTED = re.compile(r"'([^']*)'")
SYNTAX_KIND = re.compile(r"^[A-Z][a-z]+(?:[A-Z][a-z]*)+$")
NORMALIZE = [
    (re.compile(r"\(first driven at [^)]*\)"), ""),
    (re.compile(r"(combinational loop detected): .*"), r"\1"),
    (re.compile(r"(unknown macro or directive|unknown macro or compiler directive) \S+"), r"\1"),
    (re.compile(r'"[^"]*"'), '"…"'),
    (re.compile(r"\b\d+\b"), "N"),
    (re.compile(r"\s+"), " "),
]


def construct(message):
    """The diagnostic without its names; a quoted syntax kind or phrase is part of the construct and stays."""
    key = QUOTED.sub(lambda m: m.group(0) if SYNTAX_KIND.match(m.group(1)) or " " in m.group(1) else "'…'", message)
    for pat, sub in NORMALIZE:
        key = pat.sub(sub, key)
    return key.strip()


def code(text):
    """A Markdown code span that may itself hold backticks."""
    text = text.replace("|", "\\|")
    return f"`` {text} ``" if "`" in text else f"`{text}`"


def source_line(base, rel, line):
    try:
        with open(os.path.join(base, rel), errors="replace") as f:
            for i, text in enumerate(f, 1):
                if i == line:
                    text = " ".join(text.split())
                    return text if len(text) <= 110 else text[:107] + "..."
    except OSError:
        pass
    return ""


def md(text):
    return text.replace("|", "\\|")


def report(base, opts):
    path = opts.output or os.path.join(base, "results.jsonl")
    meta, recs = {}, []
    with open(path) as f:
        for line in f:
            r = json.loads(line)
            if "meta" in r:
                meta = r["meta"]
            else:
                recs.append(r)
    corpus = {d["name"]: d for d in load_corpus()}
    outcomes = ["lowered", "unsupported", "rejected", "crash", "memout", "timeout", "accepted", "invalid", "error"]
    per_design = collections.defaultdict(collections.Counter)
    for r in recs:
        per_design[r["design"]][r["outcome"]] += 1
    total = collections.Counter()
    for c in per_design.values():
        total.update(c)
    used = [o for o in outcomes if total[o]]

    # A module that is invalid as a top may still be measured inside another top.
    by_name = collections.defaultdict(dict)
    for r in recs:
        by_name[r["design"]][r["module"]] = r
    in_design_ok = collections.defaultdict(set)
    in_design_blocked = collections.defaultdict(set)
    for r in recs:
        if r["outcome"] == "lowered":
            in_design_ok[r["design"]].update(r["instantiates"])
        elif r["outcome"] in ("unsupported", "rejected"):
            in_design_blocked[r["design"]].update(d["owner"] for d in r["diags"])

    w = sys.stdout.write
    w("# Front-end census\n\n")
    w(
        "What happens when every module of a set of open RTL designs is made a top and lowered to\n"
        "the word-level netlist (`tools/quality/census.py`, designs in `tools/quality/census_corpus.json`).\n"
        f"svlang `{meta.get('svlang', '?')[:12]}`, {meta.get('date', '')}"
        + (f", reference front end {meta['reference']}" if meta.get("reference") else "")
        + (f", with `{' '.join(meta['defines'])}` defined" if meta.get("defines") else "")
        + ". Regenerate after a fix; the designs are kept outside the repository.\n\n"
    )
    measured = sum(total[o] for o in used if o != "invalid")
    w(
        f"{sum(total.values())} modules. {total['invalid']} cannot stand alone as a top (see *Invalid as a top*), "
        f"which leaves {measured} measured: **{total['lowered']} lowered "
        f"({100 * total['lowered'] / max(measured, 1):.1f}%)**, {total['unsupported']} unsupported, "
        f"{total['rejected']} wrongly rejected, {total['crash']} crashes, {total['memout']} out of memory, "
        f"{total['timeout']} timeouts, "
        f"{total['accepted']} lowered although the reference rejects the input.\n\n"
    )

    w("## Designs\n\n")
    w("| design | commit | license | modules | " + " | ".join(used) + " | lowered of measured |\n")
    w("| --- | --- | --- | ---: | " + " | ".join("---:" for _ in used) + " | ---: |\n")
    for name in corpus:
        c = per_design.get(name)
        if not c:
            continue
        n = sum(c.values())
        m = n - c["invalid"]
        d = corpus[name]
        w(
            f"| [{name}]({d['repo']}) | `{d['rev'][:10]}` | {d['license']} | {n} | "
            + " | ".join(str(c[o]) for o in used)
            + f" | {100 * c['lowered'] / max(m, 1):.0f}% |\n"
        )
    w(
        f"| **all** | | | {sum(total.values())} | "
        + " | ".join(str(total[o]) for o in used)
        + f" | {100 * total['lowered'] / max(measured, 1):.0f}% |\n\n"
    )

    def ranked(title, intro, want, kinds):
        blocked = collections.defaultdict(set)  # construct -> tops
        sole = collections.Counter()
        owners = collections.defaultdict(set)
        designs = collections.defaultdict(set)
        example = {}
        for r in recs:
            if r["outcome"] not in want:
                continue
            keys = set()
            for d in r["diags"]:
                if d["kind"] not in kinds:
                    continue
                k = construct(d["message"])
                keys.add(k)
                blocked[k].add((r["design"], r["module"]))
                owners[k].add((r["design"], d["owner"]))
                designs[k].add(r["design"])
                # The example is the diagnostic in the smallest compilation.
                if k not in example or r["files"] < example[k][0]:
                    example[k] = (r["files"], d)
            if len(keys) == 1 and len({construct(d["message"]) for d in r["diags"]}) == 1:
                sole[next(iter(keys))] += 1
        if not blocked:
            return
        w(f"## {title}\n\n{intro}\n\n")
        w("| tops blocked | only blocker | modules with it | designs | diagnostic | example |\n")
        w("| ---: | ---: | ---: | ---: | --- | --- |\n")
        rows = sorted(blocked, key=lambda k: (-len(blocked[k]), -sole[k], k))
        for k in rows[: opts.rows]:
            d = example[k][1]
            src = source_line(base, d["file"], d["line"])
            ex = f"`{d['file']}:{d['line']}`" + (f" {code(src)}" if src else "")
            w(f"| {len(blocked[k])} | {sole[k]} | {len(owners[k])} | {len(designs[k])} | {md(k)} | {ex} |\n")
        if len(rows) > opts.rows:
            rest = set()
            for k in rows[opts.rows :]:
                rest |= blocked[k]
            w(f"\n{len(rows) - opts.rows} more diagnostics, in {len(rest)} tops, are in the results file.\n")
        w("\n")

    columns = (
        "*Tops blocked* counts the modules that fail with the diagnostic when made a top, wherever below them it\n"
        "arises. *Only blocker* counts those with no other diagnostic, so fixing the row alone should free them.\n"
        "*Modules with it* counts the modules whose own text has the construct."
    )
    ranked(
        "Unsupported constructs",
        "Legal SystemVerilog that lowering does not handle yet, by how many tops each blocks.\n" + columns,
        {"unsupported", "rejected"},
        {"unsupported"},
    )
    ranked(
        "Wrongly rejected",
        "svlang reports an error and the reference front end accepts the same files: the input is good and\n"
        "svlang is wrong, or stricter than the standard asks.",
        {"rejected"},
        {"error"},
    )

    crashes = [r for r in recs if r["outcome"] in ("crash", "memout", "timeout")]
    if crashes:
        w(
            "## Crashes and limits\n\n"
            f"A top that panics, or needs more than {meta.get('timeout', '?')} s or {meta.get('mem', '?')} GB.\n\n"
        )
        groups = collections.defaultdict(list)
        for r in crashes:
            c = r.get("crash", {})
            key = (
                r["outcome"]
                if r["outcome"] != "crash"
                else f"{c.get('message', '')} ({c.get('where', '') or 'exit ' + str(c.get('exit'))})"
            )
            groups[key].append(r)
        w("| tops | what | example |\n| ---: | --- | --- |\n")
        for key, rs in sorted(groups.items(), key=lambda kv: -len(kv[1])):
            ex = min(rs, key=lambda r: r["files"])
            w(f"| {len(rs)} | {md(key)} | `{ex['file']}` `{ex['module']}` ({ex['files']} files) |\n")
        w("\n")

    accepted = [r for r in recs if r["outcome"] == "accepted"]
    if accepted:
        w(
            "## Lowered although the reference rejects the input\n\n"
            "svlang produced a netlist from input the reference front end calls an error. Either svlang misses a\n"
            "check it should make, or the reference is stricter than the simulators these designs target.\n\n"
        )
        groups = collections.defaultdict(list)
        for r in accepted:
            groups[construct(r.get("reference_error", {}).get("message", "?"))].append(r)
        w("| tops | reference error | example |\n| ---: | --- | --- |\n")
        rows = sorted(groups.items(), key=lambda kv: -len(kv[1]))
        for key, rs in rows[: opts.rows]:
            ex = min(rs, key=lambda r: r["files"])
            e = ex.get("reference_error", {})
            w(f"| {len(rs)} | {md(key)} | `{e.get('file', ex['file'])}:{e.get('line', 0)}` (top `{ex['module']}`) |\n")
        w("\n")

    invalid = [r for r in recs if r["outcome"] in ("invalid", "error")]
    if invalid:
        w(
            "## Invalid as a top\n\n"
            "Both front ends reject these: most are modules whose parameters have no usable default, or that need\n"
            "a file, a define or a generated header the census does not supply. They say nothing about svlang as\n"
            "tops. Where another top instantiates one, it is still measured there:\n\n"
        )
        seen_ok = sum(1 for r in invalid if r["module"] in in_design_ok[r["design"]])
        seen_blocked = sum(
            1
            for r in invalid
            if r["module"] not in in_design_ok[r["design"]] and r["module"] in in_design_blocked[r["design"]]
        )
        w(
            f"- {seen_ok} of the {len(invalid)} are instantiated inside a top that lowered\n"
            f"- {seen_blocked} hold a construct that blocks a top above them\n"
            f"- {len(invalid) - seen_ok - seen_blocked} are not reached by any measured top\n\n"
        )
        groups = collections.defaultdict(list)
        for r in invalid:
            groups[construct(r.get("reference_error", {}).get("message") or (r["diags"][0]["message"] if r["diags"] else "?"))].append(r)
        w("| tops | reference error | example |\n| ---: | --- | --- |\n")
        rows = sorted(groups.items(), key=lambda kv: -len(kv[1]))
        for key, rs in rows[:15]:
            ex = min(rs, key=lambda r: r["files"])
            w(f"| {len(rs)} | {md(key)} | `{ex['design']}` `{ex['module']}` |\n")
        w("\n")

    w("## Method and limits\n\n")
    w(
        "- Each module is compiled as a top with its default parameters, from the file that defines it and the\n"
        "  files that define the names it uses, found by a text scan. No design's own build flow is run, so a\n"
        "  configuration a flow would set (a define, a parameter, a generated file) is absent.\n"
        "- Lowering stops at its first failing stage, so a top's diagnostics are a lower bound: fixing a row can\n"
        "  uncover the next one.\n"
        "- A lowered netlist is not checked for correctness here; this census measures coverage, not equivalence.\n"
        "- Testbench, verification and tool directories are left out by name.\n"
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("command", choices=["fetch", "run", "report"])
    ap.add_argument("dir", help="where the designs are kept (outside the repository)")
    ap.add_argument("--only", action="append", default=[], help="one design (repeatable)")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--timeout", type=int, default=120, help="seconds per module")
    ap.add_argument("--mem", type=float, default=4.0, help="GB of address space per process")
    ap.add_argument("--define", action="append", default=[], help="a define for every compilation (repeatable)")
    ap.add_argument("--rows", type=int, default=40, help="rows per ranked table")
    ap.add_argument("-o", "--output", help="results file (default <dir>/results.jsonl)")
    opts = ap.parse_args()
    base = os.path.abspath(opts.dir)
    if opts.command == "fetch":
        fetch(base, opts.only)
    elif opts.command == "run":
        run(base, opts.only, opts)
    else:
        report(base, opts)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Regenerates the rule documentation from the rules themselves.

Each rule declares its metadata once, where the native engine registers it
(crates/svlang-lint/src/rules/<family>/mod.rs: ID, name, category, severity,
profiles, guidelines, description; sub-rules in
crates/svlang-lint/src/support/style_config.rs). This script queries the built
`svlang` binary and writes:

  docs/rules/catalog.json           machine-readable catalog (svlang lint --list-rules --json)
  docs/rules/README.md              human-readable catalog, with 3-3-3 domain/prefix matrix
  docs/methodology/MG-*.md          the "Enforced by:" line under each guideline

`tests/test_catalog.py` and `crates/svlang-lint/tests/catalog.rs` check that the
binary and `docs/rules/catalog.json` agree and obey the `DOM-GRP-NNN` taxonomy.

Usage:
  tools/codegen/gen_rule_docs.py [--binary target/release/svlang]   rewrite the files
  tools/codegen/gen_rule_docs.py --check                            exit 1 if any file is stale
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RULES_DOCS = REPO / "docs" / "rules"
GUIDE_DOCS = REPO / "docs" / "methodology"
TESTS = REPO / "testdata" / "rules"

DOMAINS = [
    ("CMP", "Compiler", "IEEE 1800-2023 preprocessor, syntax, elaboration, and semantic diagnostics"),
    ("RTL", "RTL Design", "Synthesizable RTL structure, FSM state reachability/deadlocks, CDC, and X-optimism/width hazards"),
    ("SVL", "SystemVerilog", "Language correctness, types, DPI-C, OOP, concurrency, randomization, coverage, performance, dead code, maintainability, security, style, naming, and waivers"),
    ("SVA", "Assertions & Formal", "Concurrent/immediate SVA properties, clocks, resets, action blocks, vacuity, and pre-FPV formal readiness"),
    ("UVM", "IEEE 1800.2 UVM", "Factory, phasing, objections, uvm_config_db, sequences, TLM/scoreboards, component hierarchy/roles, RAL register models, and UVM naming/migration"),
]

PREFIX_DESCRIPTIONS = {
    "CMP-ERR": "Preprocessor, parser, elaboration, and type-checking diagnostics",
    "RTL-SYN": "Synthesizable sequential/combinational blocks, resets, gated clocks, latches, and clocked races",
    "RTL-FSM": "FSM enum exhaustiveness, unreachable states, and deadlock states",
    "RTL-CDC": "Unsynchronized multi-bit and combinational clock-domain crossings",
    "RTL-DAT": "Case defaults, casex/X-equality hazards, reversed part-selects, and concat literal widths",
    "SVL-COR": "General SV correctness, branch/copy-paste bugs, subroutine CFG dataflow, error checks, and timescales",
    "SVL-TYP": "Signed/unsigned mixing, enum/array/struct casts and patterns, and loop counter hazards",
    "SVL-OOP": "Virtual dispatch, $cast safety, constructors, shallow-copy aliasing, shadowing, and SOLID contracts",
    "SVL-CON": "fork/join lifetime hazards, event/trigger races, semaphores/mailboxes, clocking blocks, and interface drives",
    "SVL-RND": "Unchecked randomize(), unsatisfiable/contradictory/soft/dist constraints, solver blow-up, and seeding",
    "SVL-COV": "Unreferenced/unsampled covergroups, cross-bin explosion, bin overlaps, and per_instance options",
    "SVL-PRF": "By-value container copies, O(N^2) queue/string loops, #0 delta thrashing, and dynamic allocation in loops",
    "SVL-DED": "Unused variables, arguments, parameters, typedefs, macros, class properties, dead stores, and unreachable code",
    "SVL-MNT": "Complexity metrics, design/package smells, code/constraint clones, readability, and git-history hotspots",
    "SVL-WVR": "Unclosed block waivers and invalid or unused waiver hygiene diagnostics",
    "SVL-PRT": "Vendor-specific system tasks, hardcoded host paths, $unit pollution, and header macro leaks",
    "SVL-DPI": "Conflicting DPI-C C prototypes and non-portable 4-state DPI argument types",
    "SVL-SEC": "Plaintext cryptographic secret logging and unchecked $fopen descriptors",
    "SVL-DEP": "Deprecated program/defparam/always/$random/assign/non-ANSI constructs and legacy reg declarations",
    "SVL-STY": "End labels, whitespace, line length, banners, include guards, macro parens, and configurable style engine",
    "SVL-NAM": "Package, interface, typedef, macro, constraint, covergroup, and configurable SV naming conventions",
    "SVA-PRP": "SVA clocks, resets, action-block side effects, vacuous antecedents, unbounded delays, and interface contracts",
    "SVA-FPV": "Pre-FPV formal-readiness checks: output/contradictory assumes, reset-less control flops, and state-space bounds",
    "UVM-FAC": "UVM factory registration macros, parameterized utils, type-safe overrides, and parent handles",
    "UVM-PHS": "Phase signatures, super.<phase> ordering, zero-time function-phase rules, and runtime domain mixing",
    "UVM-OBJ": "Inter-procedural raise/drop objection CFG balance, loop/fork objection hazards, and drain delays",
    "UVM-CFG": "uvm_config_db type/path resolution, wildcard startup cost, runtime misuse, and config objects",
    "UVM-SEQ": "start_item/finish_item handshakes, sequence/sequencer typing, p_sequencer, virtual sequencers, and item hooks",
    "UVM-TLM": "TLM port/export/FIFO connectivity, analysis transaction mutation, FIFO starvation, and scoreboard checks",
    "UVM-CMP": "Component layering, Law of Demeter, singletons, callbacks, UVM reporting, agent/monitor/driver roles, and passive VIF drives",
    "UVM-RAL": "Register block locking, address/field overlaps, orphan registers, adapter/predictor wiring, and mirror tampering",
    "UVM-DEP": "Deprecated UVM 1.1d/1.2/1800.2 APIs, starting_phase, uvm_test_done, `uvm_field_*, and `uvm_do* macros",
    "UVM-PRT": "Non-IEEE Accellera-only (@uvm-accellera) and @uvm-contrib UVM library dependencies",
    "UVM-NAM": "UVM component, sequence, item, TLM port, create() instance string, and member prefix naming conventions",
}


def run_json(binary, *args):
    cmd = [binary]
    if Path(binary).stem == "svlang":
        cmd.append("lint")
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "out.json"
        res = subprocess.run([*cmd, *args, "--json", str(out)], capture_output=True, text=True,
                             cwd=tmp)
        if res.returncode != 0:
            sys.exit(f"{' '.join(cmd)} {' '.join(args)} failed:\n{res.stderr}")
        return out.read_text()


def test_dirs():
    """{rule id: path of its test directory relative to docs/rules}."""
    return {p.name: Path("..", "..", p.relative_to(REPO)).as_posix()
            for p in TESTS.glob("*/*") if p.is_dir()}


def cat_anchor(cat):
    return cat.lower().replace("_", "-")


def render_markdown(catalog):
    tests = test_dirs()
    rules = catalog["rules"]
    by_cat = {c: [] for c in catalog["categories"]}
    by_dom = {d[0]: [] for d in DOMAINS}
    by_prefix = {}
    for r in rules:
        by_cat.setdefault(r["category"], []).append(r)
        dom, grp, _ = r["id"].split("-")
        prefix = f"{dom}-{grp}"
        by_dom.setdefault(dom, []).append(r)
        by_prefix.setdefault(prefix, []).append(r)

    lines = [
        "# svlang-lint Rule Catalog",
        "",
        "<!-- Generated by tools/codegen/gen_rule_docs.py from the rules' own metadata. Do not edit. -->",
        "",
        f"**{len(rules)} rules and sub-rules** across **5 domains**, **{len(by_prefix)} prefixes**, and "
        f"**{len(catalog['categories'])} categories** using the strict 2-tier **`DOM-GRP-NNN`** (`3-3-3`) taxonomy.",
        "",
        "- **Inspect any rule**: `svlang lint --explain <ID-or-Name>` prints the rule description, options, guidelines, and tested `bad.sv` / `good.sv` examples.",
        "- **Select or waive by prefix wildcard**: `--rule 'RTL-*'`, `--rule 'SVL-CON-*,SVA-*'`, `svlang.lint.ignore: [\"SVL-NAM-*\", \"UVM-NAM-*\"]`, or inline `// svlang-lint:off SVL-STY-*`.",
        "- **Machine-readable catalog**: `svlang lint --list-rules --json -` (published at [`catalog.json`](catalog.json)).",
        "",
        "## Domains (`DOM-*`)",
        "",
        "| Selector | Domain | Prefixes | Rules | Coverage |",
        "| :--- | :--- | :--- | ---: | :--- |",
    ]
    for dom, title, summary in DOMAINS:
        rs = by_dom.get(dom, [])
        prefixes = sorted({f"{r['id'].split('-')[0]}-{r['id'].split('-')[1]}" for r in rs})
        p_str = ", ".join(f"`{p}`" for p in prefixes)
        lines.append(f"| `{dom}-*` | **{title}** | {p_str} | {len(rs)} | {summary} |")

    lines += [
        "",
        "## Prefix & Category Matrix (`DOM-GRP-*`)",
        "",
        "| Prefix | Category | ID Range | Rules | Scope |",
        "| :--- | :--- | :--- | ---: | :--- |",
    ]
    dom_order = {d[0]: i for i, d in enumerate(DOMAINS)}
    cat_order = {c: i for i, c in enumerate(catalog["categories"])}
    ordered_prefixes = sorted(
        by_prefix.keys(),
        key=lambda p: (dom_order.get(p.split("-")[0], 99), cat_order.get(by_prefix[p][0]["category"], 99), p),
    )
    for pref in ordered_prefixes:
        rs = sorted(by_prefix[pref], key=lambda r: r["id"])
        cat = rs[0]["category"]
        id_range = f"`{rs[0]['id']}`" if len(rs) == 1 else f"`{rs[0]['id']}` .. `{rs[-1]['id']}`"
        scope = PREFIX_DESCRIPTIONS.get(pref, "")
        lines.append(f"| `{pref}-*` | [{cat}](#{cat_anchor(cat)}) | {id_range} | {len(rs)} | {scope} |")

    lines += [
        "",
        "## Categories",
        "",
        "| Category | Rules |",
        "| :--- | ---: |",
    ]
    for cat, rs in by_cat.items():
        if rs:
            lines.append(f"| [{cat}](#{cat_anchor(cat)}) | {len(rs)} |")
    for cat, rs in by_cat.items():
        if not rs:
            continue
        lines += ["", f"## {cat}", "", "| ID | Name | Severity | Profiles | Guidelines | Description |",
                  "| :--- | :--- | :--- | :--- | :--- | :--- |"]
        for r in rs:
            rid = f"[`{r['id']}`]({tests[r['id']]})" if r["id"] in tests else f"`{r['id']}`"
            desc = r["description"].replace("|", "\\|").replace("\n", " ")
            if r.get("reportedBy"):
                desc += f" (reported by `{r['reportedBy']}`)"
            if r.get("maturity") == "experimental":
                desc = "**Experimental** (runs with `--experimental`). " + desc
            lines.append(f"| {rid} | {r['name']} | {r['severity']} | {', '.join(r['profiles'])} | "
                         f"{', '.join(r['guidelines'])} | {desc} |")
    return "\n".join(lines) + "\n"


def render_guide_page(text, rules_by_guideline):
    out, current = [], None
    for line in text.splitlines():
        m = re.match(r"^## (MG-\d+\.\d+) ", line)
        if m:
            current = m.group(1)
        elif current and line.startswith("Enforced by:"):
            rules = rules_by_guideline.get(current, [])
            line = ("Enforced by: " + ", ".join(f"`{r}`" for r in rules)) if rules \
                else "Enforced by: review only (no rule yet)."
            current = None
        out.append(line)
    return "\n".join(out) + ("\n" if text.endswith("\n") else "")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--binary", help="the native svlang binary (default: the newer of target/release and target/debug)")
    ap.add_argument("--check", action="store_true", help="report stale files instead of writing them")
    args = ap.parse_args()
    built = [p for p in (REPO / "target" / "release" / "svlang", REPO / "target" / "debug" / "svlang") if p.exists()]
    built.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    if not args.binary and not built:
        sys.exit("no native svlang built: run `cargo build -p svlang-cli`, or pass --binary")
    binary = str(Path(args.binary or built[0]).resolve())

    catalog_text = run_json(binary, "--list-rules")
    catalog = json.loads(catalog_text)
    guide = json.loads(run_json(binary, "--list-guidelines"))["guidelines"]
    by_guideline = {g["id"]: g["rules"] for g in guide}

    wanted = {
        RULES_DOCS / "catalog.json": catalog_text,
        RULES_DOCS / "README.md": render_markdown(catalog),
    }
    for page in sorted(GUIDE_DOCS.glob("MG-*.md")):
        wanted[page] = render_guide_page(page.read_text(), by_guideline)

    stale = [p for p, text in wanted.items() if not p.exists() or p.read_text() != text]
    if args.check:
        for p in stale:
            print(f"stale: {p.relative_to(REPO)} (run tools/codegen/gen_rule_docs.py)")
        return 1 if stale else 0
    for p in stale:
        p.write_text(wanted[p])
        print(f"wrote {p.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

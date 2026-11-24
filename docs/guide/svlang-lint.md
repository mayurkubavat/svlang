# svlang lint guide

**`svlang lint`** (also available via a `svlang-lint` symlink) is a fast, pure-Rust static analysis and linting engine designed for modern **IEEE 1800-2023 (SystemVerilog)** and **IEEE 1800.2 (UVM)** design verification codebases. Built directly on `svlang`'s IEEE 1800-2023 compiler front end and `svlang-model`, it provides deep semantic and CST analysis, factory and phasing validation, simulation-race detection, formal-readiness checks, and automated fix-it capabilities with zero C++ dependencies.

---

## Key Features

- **475 Rules & Sub-Rules in a 2-Tier `DOM-GRP-NNN` (`3-3-3`) Taxonomy** ([catalog](../rules/README.md)): 5 top-level domains (`CMP`, `RTL`, `SVL`, `SVA`, `UVM`), 34 3-letter groups, and 18 categories covering RTL/FSM/CDC, SVA & formal readiness (`SVA-FPV-*`), UVM methodology & RAL, concurrency & delta-cycle races, solver constraints, coverage, OOP, performance, security, and style.
- **Prefix Wildcard Targeting**: Select, auto-fix, ignore, or waive whole domains and groups (`--rule 'RTL-*'`, `--rule 'SVL-CON-*,SVA-*'`, `--fix-rule 'SVL-STY-*'`, `// svlang-lint:off SVL-STY-*`).
- **Deep Semantic Graph Analyzers**:
  - **Objection Control Flow Graph (CFG)**: Verifies every `raise_objection()` path strictly reaches a matching `drop_objection()` across routines and branches without early-return leaks or asymmetric divergence (`UVM-OBJ-001`, `UVM-OBJ-003`, `UVM-PHS-006`).
  - **Config DB Cross-Component Resolver**: Matches `set()` and `get()` calls across hierarchy strings to detect type mismatches and unconfigured lookups (`UVM-CFG-001`, `UVM-CFG-005`, `UVM-CFG-012`).
  - **TLM Port-to-Export Elaboration Checker**: Detects unmapped driver pull ports, unconnected analysis ports, and starved analysis FIFOs across `connect_phase()` (`UVM-TLM-001`, `UVM-TLM-003`, `UVM-TLM-005`).
- **Sub-Second Parallel Execution**: Pure-Rust multi-core scheduler analyzing large RTL and UVM environments with deterministic output order and incremental `.svlang_lint_cache/` reuse.
- **Automated Fix-It Engine (`--fix`)**: Applies simulation-preserving safe rewrites automatically; behavior-changing fixes require `--unsafe-fixes`, and display-only hints are shown without modifying files.
- **Zero-Regression Baselines & Scoped Waivers**: Fingerprint-based `.svlang-lint/baseline.json`, expiring/re-grading waivers in `.svlang-lint/config.py` / `waivers.json`, and Verissimo-compatible inline/block waivers.
- **CI/CD & IDE Integration**: Emits SARIF 2.1.0, JSON, self-contained HTML dashboards, and powers `svlang lsp` + the [`svlang.svlang`](../../editors/vscode/README.md) VS Code / Open VSX extension.

---

## `DOM-GRP-NNN` Rule Taxonomy

Every diagnostic ID follows **`DOM-GRP-NNN`** (3-letter domain, 3-letter group, 3-digit number):

| Domain | Selector | Groups (`DOM-GRP`) | Categories | What it checks |
| :--- | :--- | :--- | :--- | :--- |
| **`CMP`** | `CMP-*` | `CMP-ERR` | `Compiler` | Preprocessor, parser, elaboration, and IEEE 1800 semantic errors (`CMP-ERR-001`) & warnings (`CMP-ERR-002`) |
| **`RTL`** | `RTL-*` | `RTL-SYN`, `RTL-FSM`, `RTL-CDC`, `RTL-DAT` | `RTL` | Reset polarity/completeness, gated clocks, inferred latches, clocked blocking races, FSM deadlocks/unreachable states, CDC, `casex`/X-equality, part-selects |
| **`SVL`** | `SVL-*` | `SVL-COR`, `SVL-TYP`, `SVL-OOP`, `SVL-CON`, `SVL-RND`, `SVL-COV`, `SVL-PRF`, `SVL-DED`, `SVL-MNT`, `SVL-WVR`, `SVL-PRT`, `SVL-DPI`, `SVL-SEC`, `SVL-DEP`, `SVL-STY`, `SVL-NAM` | `Correctness`, `Types`, `OOP`, `Concurrency`, `Randomization`, `Coverage`, `Performance`, `DeadCode`, `Maintainability`, `Portability`, `Security`, `Deprecated`, `Style`, `Naming` | SV expressions & CFG dataflow, OOP & `$cast`, `fork`/`join` & clocking blocks, constraint solver soundness, covergroups, container performance, dead code, complexity/clones, DPI-C, security, style & naming |
| **`SVA`** | `SVA-*` | `SVA-PRP`, `SVA-FPV` | `SVA` | Concurrent/immediate SVA clocks, resets, action blocks, vacuous antecedents, and pre-FPV formal readiness (`SVA-FPV-001..005`) |
| **`UVM`** | `UVM-*` | `UVM-FAC`, `UVM-PHS`, `UVM-OBJ`, `UVM-CFG`, `UVM-SEQ`, `UVM-TLM`, `UVM-CMP`, `UVM-RAL`, `UVM-DEP`, `UVM-PRT`, `UVM-NAM` | `UVM`, `Deprecated`, `Portability`, `Naming` | Factory, phasing, objections, `uvm_config_db`, sequences/sequencers, TLM/scoreboards, component layering/roles, RAL register blocks/maps, UVM deprecations & naming |

> Every rule, with its category, severity, presets, guidelines and links to its `bad.sv`/`good.sv` examples, is listed in the generated [Rule Catalog](../rules/README.md) (`svlang lint --list-rules --json -` gives the same data as JSON).

---

## Getting Started

### Installing or Building `svlang`

Install from crates.io, download a release tarball, or build from source (requires Rust `>= 1.82`):

```bash
# Option 1: Install from crates.io
cargo install svlang-cli

# Option 2: Build from source
git clone https://github.com/mayurkubavat/svlang.git
cd svlang
cargo build --release -p svlang-cli
export PATH="$(pwd)/target/release:$PATH"
```

The unified binary is at `target/release/svlang`. Symlinking `svlang-lint -> svlang` also dispatches directly into `svlang lint`.

---

## Running the Linter

### Basic Lint
```bash
# Lint a SystemVerilog source file with include directory
svlang lint path/to/my_env.sv -I path/to/includes

# Lint a package with multiple source files
svlang lint tb/top_pkg.sv tb/my_agent.sv tb/my_scoreboard.sv
```

### Rule Profiles & Presets
Filter rule execution using curated verification profiles:
With no preset, the rules that find bugs and methodology problems run:
the `soundness`, `methodology`, `performance` and `security` profiles
(`--preset default` says the same). Style, naming and layout rules
(`style`), maintainability metrics (`smells`) and the opinionated
`advisory` rules are opt-in: name the preset, use `--preset all`, set
`preset` in the project config, or configure the rules you want there (a
configured rule runs whatever the preset).

```bash
# Run only soundness checks (concurrency, SVA, correctness, types)
svlang lint tb/top.sv --preset soundness

# Run only UVM methodology checks
svlang lint tb/top.sv --preset methodology

# Run pre-FPV formal-readiness checks (SVA-FPV-001..005)
svlang lint tb/top.sv --preset formal-ready

# Run only performance optimizations
svlang lint tb/top.sv --preset performance

# Run only code style checks
svlang lint tb/top.sv --preset style

# Run only maintainability checks (metrics, design smells)
svlang lint tb/top.sv --preset maintainability

# Run the opt-in advisory rules (magic numbers, commented-out code, ...)
svlang lint tb/top.sv --preset advisory

# Moving to IEEE 1800.2 UVM: deprecated and banned API checks only
svlang lint tb/top.sv --preset uvm-1800.2

# Run everything: style, smells and advisory rules included
svlang lint tb/top.sv --preset all
```

`--rule <ID>` always runs the named rule regardless of preset.

### Methodology Guide

The [UVM Methodology Guide](../methodology/README.md) groups guidelines for reusable, maintainable
testbenches into nine chapters (layering, coupling, component roles, reuse, design patterns, construct
usage, duplication, failure handling, evolvability), each backed by lint rules:

```bash
svlang lint --list-guidelines                 # the guide and the rules behind each guideline
svlang lint tb/top.sv --guideline MG-4         # one chapter (or MG-4.1 for one guideline)
svlang lint tb/top.sv --preset reuse           # chapters MG-1 to MG-5
svlang lint tb/top.sv --conformance            # pass/fail per guideline after any run
svlang lint tb/top.sv --guideline MG-9.2 --history  # hotspots and change coupling from git
```

### Rule & Prefix Wildcard Targeting
Run or debug a single rule, a comma-separated list, or an entire `DOM-*` / `DOM-GRP-*` prefix in milliseconds:
```bash
svlang lint rtl/alu.sv --rule 'RTL-*'
svlang lint tb/top.sv  --rule 'SVL-CON-*,UVM-OBJ-*'
svlang lint tb/my_seq.sv --rule UVM-DEP-006
svlang lint tb/my_scb.sv --rule ScoreboardLeftoverQueue
```

### Compiler errors and warnings
The front end's own diagnostics are findings too: errors are `CMP-ERR-001` and warnings `CMP-ERR-002`. They appear in the console, JSON, SARIF, HTML and the editor, and can be waived or baselined like any other finding.

### Findings in macros
A finding in code that a macro produced is reported at the line that uses the macro (column 0), because that is the line you can waive or change, with a note naming the macro and pointing at the place in its body the finding is about:

```
tb/env.sv:10:0: [ERROR] SVL-COR-004 (SelfComparison): Self-comparison detected: ...
    note: tb/defs.svh:4:11: in the expansion of macro 'SET_ALL'
```

The note is in the console, JSON (`related`), SARIF (`relatedLocations`) and the editor. Findings on a macro's arguments are code you wrote at the call and have no note.

### Exit status
`svlang lint` exits with:

| Status | Meaning |
|---|---|
| 0 | No ERROR finding. |
| 1 | At least one ERROR finding, compiler errors (`CMP-ERR-001`) included; also `--strict-new-files` when a new file has findings. |
| 2 | The run could not start: an unknown option, a source, file list (nested ones too), library file or `-y` directory that is missing or cannot be read (a directory or device given as a source included), or a configuration that fails to load. A missing `-I` / `+incdir+` directory is not an error. Unknown simulator options (`+acc`, `-l run.log`) are ignored. |
| 4 | `--strict-waivers` found unused, expired or stale waivers. |

A file that reads but does not parse is a finding (status 1), not status 2.

### Performance & Threading
Rules run in parallel across all available cores by default (override with `SVLANG_JOBS=N`).
Findings are printed in deterministic file/line order and are identical for any thread count.
```bash
# Limit worker threads
SVLANG_JOBS=4 svlang lint tb/top.sv

# Show timing breakdown
svlang lint tb/top.sv --time
```

Results are cached in `.svlang_lint_cache/` and reused when the tool version, the rule set and its options, waivers, defines, include directories, and each file's includes and imported packages are unchanged. Whole-program rules (duplication, package layering, TLM and register wiring) are reused only when every input is unchanged. The cache is on by default; a corrupt or mismatched entry is ignored and the run is done from scratch. See [result_cache.md](result_cache.md).

```bash
svlang lint tb/top.sv                 # writes or reuses .svlang_lint_cache/
svlang lint tb/top.sv --no-cache      # always analyze
svlang lint tb/top.sv --clear-cache   # delete the cache, then run
svlang lint tb/top.sv --cache-dir /tmp/svlang-lint-cache
```

### Automated Fix-It Rewrites
Each fix is `safe`, `unsafe`, or `display-only`. `--fix` writes only safe fixes (end labels, whitespace, rewrites that keep the same simulation behaviour). `casex` to `casez`, `==` to `===`, scheduling changes, and edits that touch factory names, `config_db`, objections, or phase order are unsafe: they are reported, and applied only with `--unsafe-fixes`. A display-only fix (for example a suggested `at_least` value) is shown in JSON, SARIF, HTML, and the editor, and is never written automatically. Unsafe fixes that were left alone are counted: `N fixes applied, M unsafe fixes skipped (use --unsafe-fixes)`. Style fixes (safe unless noted): end labels (SVL-STY-003/006/104), trailing whitespace and tabs (SVL-STY-012/101/102), a space between a control keyword and `(` (SVL-STY-010), one statement per line at the same indent (SVL-STY-009), the `;` after a `` `uvm_* `` macro call (SVL-STY-001), the width of an unsized based literal such as `'hFF` to `32'hFF` (SVL-STY-008; not for `x`/`z` digits or plain decimals, whose signedness would change), and an `` `ifndef `` include guard around a header (SVL-STY-011, unsafe: a header meant to be included twice stops working). Naming rules have no fix, since a rename must update every use across files. UVM fixes (all unsafe): a missing `` `uvm_object_utils `` / `` `uvm_component_utils `` is added after the class header (UVM-FAC-003, not for parameterized classes), a missing `super.<phase>(phase)` becomes the phase's first statement after its declarations (UVM-PHS-001), and a `$display("fmt", ...)` statement becomes `` `uvm_info(get_type_name(), $sformatf("fmt", ...), UVM_LOW) `` (UVM-CMP-009, instance methods only).
```bash
svlang lint tb/legacy_code.sv --fix-dry-run   # print the diff, change nothing
svlang lint tb/legacy_code.sv --fix           # rewrite safe fixes
svlang lint tb/legacy_code.sv --fix --unsafe-fixes   # also apply behaviour-changing fixes
svlang lint tb/legacy_code.sv --fix --fix-rule RTL-DAT-002,SVL-STY-003   # only these rules' fixes
```
Fixes land in the file that holds the code, including included `.svh` files. Waived and baselined findings are not fixed. A file edited after it was read is skipped, and a file with compile errors is never rewritten. Fixes inside macro expansions are left alone, and a fix whose replacement would drop a macro call the code wrote (for example `` d[1+`MSB:7] `` rewritten from the expansion) becomes display-only. One `--fix` run repeats passes until nothing changes: overlapping fixes and findings that a fix uncovers are handled in the next pass, and the final report shows what is left in the fixed code. It stops after 10 passes, or as soon as fixes start undoing each other, with a warning. `tests/test_fix_invariants.py` checks on every rule test file, and on mutated copies, that `--fix` never crashes or hangs, adds no compile errors, keeps every macro call, and settles in one run. Each file is written to a temporary copy and renamed into place. In the editor a safe fix is the preferred quick fix; unsafe and display-only fixes are offered as ordinary code actions.

### Reporting (SARIF, JSON & HTML)
```bash
# Machine-readable findings for scripts and CI ('-' writes to stdout)
svlang lint tb/*.sv --json results.json

# Generate SARIF 2.1.0 for CI/CD code scanning
svlang lint tb/*.sv --sarif results.sarif

# Generate interactive HTML dashboard
svlang lint tb/*.sv --html report.html
```

### Comparing and merging reports
`--json` reports carry a fingerprint per finding that survives edits elsewhere in the file, so runs can be compared:
```bash
python3 -m svlang_lint.report diff  main.json branch.json --fail-on-new     # new and fixed findings
python3 -m svlang_lint.report merge blk_a.json blk_b.json -o all.json       # one report from several jobs
python3 -m svlang_lint.report trend w1.json w2.json w3.json -o trend.html   # burn-down by category, guideline, block, rule
```
Run them from `python/`, or `pip install ./python` and use `svlang-lint-report`. JSON findings also name their rule's `category`.

### Custom Python Rules Plugin SDK
Verification teams can author custom corporate or project verification checks in pure Python without recompiling Rust:
```bash
# Execute custom Python rules from a file or directory
svlang lint tb/*.sv --python-rules python/examples/custom_naming_rules.py

# Python rule violations seamlessly participate in SARIF, HTML, and baselines
svlang lint tb/*.sv --python-rules my_rules/ --sarif results.sarif
```
See [Python Rule SDK Documentation](python_rule_sdk.md) for full API reference, context query helpers, and tutorials.

### Explain a rule
`--explain SVL-MNT-022` (ID or name) prints what the rule checks, the methodology guidelines
it enforces, the options it reads with their defaults, and a flagged and a fixed example.
The examples are the rule's regression tests (`testdata/rules/<category>/<ID>/`), compiled into
the binary at build time, so they always match what the rule does.

### Testbench map
`--map tb_map.html` draws the testbench as the linter sees it: the component tree from the
test down (which class creates which), each component's TLM ports and `connect()` calls,
and the config_db fields set and read, with ports that are never connected, gets without a
set and sets nobody reads marked. Findings are attached to the component class they fall
in. Use a `.json` path for the same data in machine-readable form.

### Language Server Protocol (LSP) & IDE Integration
Run `svlang` as an in-editor language server for VS Code, Cursor, Antigravity, Neovim, or any LSP client:
```bash
# Language server over stdin/stdout (`svlang lint --lsp` starts the same server)
svlang lsp
```
`svlang lsp` provides:
- **Compiler-backed navigation & highlighting**: semantic tokens, inactive `` `ifdef `` region dimming, document outline, workspace symbols, go-to-definition/type-definition, find references, Type Hierarchy, Call Hierarchy, **RTL Module Hierarchy** (`svlang/rtlHierarchy`), and **Signal Driver/Load Tracing** (`svlang/traceSignal`).
- **Mode 1 default workflow**: in the VS Code / Open VSX extension (`svlang.svlang`), `svlang.lint.trigger` defaults to `"off"` with `svlang.diagnostics.syntaxAndSemantics = true` — giving instant code browsing and live IEEE 1800-2023 syntax/semantic checks (`CMP-ERR-001`) out of the box, while full static lint rules run on demand (`Lint Now` in the Status Bar) or automatically when switched to `"onSave"` / `"onType"`.
- **Quick fixes & waivers**: safe/unsafe rule fixes, `source.fixAll.svlang`, inline `// svlang-lint-disable-next-line <RULE>` insertion, and one-click `Ignore <RULE>` in Workspace or User settings.

Project context comes automatically from `.svlang-lint/config.py` (or editor settings `svlang.compile.{filelists,includeDirs,defines}` when no repo config is present), evaluating unsaved editor buffers in memory via `svlang::syntax::overlay`:

```python
class Project(ProjectConfig):
    sources = ["$UVM_HOME/src/uvm_pkg.sv", "tb/tb_pkg.sv", "tb/tb_top.sv"]
    include_dirs = ["$UVM_HOME/src", "tb"]
    defines = ["UVM_NO_DPI"]
```

See [`editors/vscode/README.md`](../../editors/vscode/README.md) for the full extension guide.

---

### CI, pre-commit and editor
**GitHub Action.** The repository is a composite action: it gets the native `svlang-lint` (a release when `version` names one, else built from the action's source with cargo and cached; no C++ toolchain either way), lints, and uploads SARIF to code scanning.
```yaml
permissions: { contents: read, security-events: write }
steps:
  - uses: actions/checkout@v4
    with: { fetch-depth: 0 }            # needed for changed-since
  - uses: mayurkubavat/svlang@main
    with:
      changed-since: origin/main         # optional: only findings on changed lines
      args: ""                           # optional: sources/-f lists; empty uses .svlang-lint/config.py
      version: ""                        # optional: a release to download, such as v0.2.0
```

**pre-commit.** Add to `.pre-commit-config.yaml` (svlang-lint on `PATH`); `svlang-lint` checks lines changed since `HEAD`, `svlang-lint-full` lints the whole project on push:
```yaml
- repo: https://github.com/mayurkubavat/svlang
  rev: main
  hooks: [{ id: svlang-lint }, { id: svlang-lint-full }]
```

**Release builds.** Tagged releases attach `svlang-<platform>.tar.gz` (Linux x64/arm64 on glibc 2.28 or later, macOS arm64/x64, Windows x64) with the unified `svlang` binary (`check`, `lint`, `lsp`, `filelist`, `map`, `hier`, `netlist`, `btor2`, `aiger`, `yosys`) and the Python SDK; unpack and put `bin/` on `PATH`. `tools/release/package.sh` and `smoke.sh` build and check the same archive locally.

**VS Code / Open VSX.** Tagged releases build a self-contained `svlang-<target>.vsix` (`svlang.svlang`) for all 5 platforms (`linux-x64`, `linux-arm64`, `darwin-arm64`, `darwin-x64`, `win32-x64`) with `svlang` bundled inside, attach them to the GitHub release, and publish to Open VSX and VS Code Marketplace; see [editors/vscode](../../editors/vscode/README.md).

## Project Configuration

On an existing testbench, start with `svlang-lint --init` in its root folder. It
finds the sources (or a `.f` file list) and the UVM library, learns the naming
conventions your code already follows (for example drivers ending in `_bfm`
rather than `_driver`), writes `.svlang-lint/config.py`, and records today's
findings in `.svlang-lint/baseline.json` so the next run shows only new ones.
Review the generated config; it is ordinary Python you can edit.

Describe your project once in `.svlang-lint/config.py` or `svlang_lint_config.py` (auto-discovered from the working directory upward):

```python
from svlang_lint.config import ProjectConfig, RuleConfig, Waiver, Severity

class Project(ProjectConfig):
    library_paths = ["$UVM_HOME", "vip/**"]   # compiled, never reported
    exclude_paths = ["**/generated/**"]

    class MaxLineLength(RuleConfig):
        max_columns = 140

    class NoCasex(RuleConfig):
        rule = "RTL-DAT-002"
        severity = Severity.Error

    waivers = [Waiver("UVM-OBJ-002", path="tb/stress_seq.sv", line=142, reason="Stress bench")]
```

Linting a testbench against the real Accellera UVM library with `library_paths = ["$UVM_HOME"]` drops the ~18,000
findings inside UVM itself, including code generated by UVM macros in your classes, and leaves only your own code.
Teams can subclass a shared base config and override single rules. See [Project Configuration](project_config.md)
for globs, rule options, the JSON form, and `--library` / `--exclude` flags.

## Baseline & Waiver Management

### Adopting on Existing Legacy Codebases
Establish a zero-new-defects baseline without fixing thousands of pre-existing legacy violations:

```bash
# 1. Snapshot all current legacy violations into baseline JSON
svlang lint tb/*.sv --update-baseline .svlang-lint/baseline.json

# 2. Enforce zero regressions on new PRs (only new violations fail CI)
# Note: .svlang-lint/baseline.json is auto-discovered if present!
svlang lint tb/*.sv
```

Baseline entries are matched by a fingerprint of the rule, the file's path relative to the
project root (the folder holding `.svlang-lint/`) and the text of the flagged line, so adding or
removing lines elsewhere in a file, or checking the project out somewhere else, does not
bring old findings back. Editing the flagged line does: that finding is treated as touched
and must be fixed or re-baselined. Entries that no longer match anything are reported so the
baseline only shrinks; `--update-baseline` drops them.

Inline waivers go on the flagged line (`// svlang-lint:waive SVL-COR-004 reason`) or on the line
above it (`// svlang-lint-disable-next-line SVL-COR-004`). A quoted regular expression limits
the waiver to diagnostics whose message matches it (`// svlang-lint:waive SVL-COR-004 "expression 'a'"`
or `// svlang-lint: waive SVL-COR-004 "expression 'a'"`). The same `message` field is accepted on
a waiver in `config.py` and in JSON.

### Waivers Configuration
Document approved exceptions in `.svlang-lint/waivers.json` (auto-discovered) or in your `config.py`:
```json
{
  "version": "1.0",
  "waivers": [
    {
      "rule": "SVL-STY-001",
      "path": "*legacy_block*",
      "reason": "Permit trailing semicolons in legacy third-party macros"
    },
    {
      "rule": "UVM-OBJ-002",
      "file": "tb/stress_seq.sv",
      "line": 142,
      "reason": "Objection in loop verified for stress bench iteration",
      "owner": "mayur"
    }
  ]
}
```
Run with waivers automatically discovered:
```bash
svlang lint tb/*.sv
```
(Or pass `--waivers path/to/waivers.json` explicitly).

### New code is clean, without a baseline
`--changed-since <git-ref>` reports only findings on lines added or changed since that
revision (the hunks of `git diff -U0`, not every line of a touched file), plus everything
in untracked files. `--strict-new-files` keeps that filter and exits 1 when a file that
did not exist at the revision — added in the diff, or untracked — has any finding.
Warnings on edited lines of a file that already existed stay warnings and do not fail
the run. In a pull request job:

```bash
svlang lint --changed-since origin/main --strict-new-files
```

### Keeping suppressions honest
Waivers and baselines pile up unless something prunes them. `--waiver-report` lists
waivers that matched nothing in this run, expired waivers, waivers without an owner,
waiver comments in your sources that no longer waive anything, baseline entries whose
finding is gone, and how many findings are waived per rule and per owner.
`--report-unused-waivers` also records each unused or expired waiver as a `SVL-WVR-002`
warning in the finding list and the summary. It is off by default, because a shared waiver
file is often applied to runs that do not contain every waived file. `--strict-waivers`
turns those warnings on, prints the same hygiene report, and exits with status 4 while any
unused, expired or stale entry remains:

```bash
svlang lint --strict-waivers   # in CI, after the normal lint step
```

---

### Rule precision on real code
`tools/quality/error_budget.py` holds ERROR to a higher standard: on reviewed, simulator-clean code (uvm-core 2020.3.1) an ERROR finding is a false positive until shown otherwise, so each rule has a budget of them in `tools/quality/error_budget.json` that CI checks on every change and that can only go down. The aim is an empty budget.

`tools/quality/precision.py` lints a fixed corpus (`tools/quality/precision_corpus.json`, uvm-core by default) with every rule and records findings per rule per kLOC. Given the previous result it flags rules whose count jumped (+50% and at least +5), dropped to zero, or that are new and report 2 or more per kLOC. The nightly `Precision` workflow runs it and shows the table in the job summary.

New rules can set `experimental: true` in their registration metadata: they stay off unless `--experimental` or `--rule` names them, are marked in the catalog and in `--explain`, and are listed separately in the precision summary until they are promoted.

## Regression & Unit Testsuite

```bash
# Run all Rust unit, rule, and CLI integration tests
cargo test

# Run the paired per-rule test suite (testdata/rules/)
cargo test -p svlang-lint -- rules

# Run end-to-end Python CLI and --fix invariant suites
python3 tests/test_fix_invariants.py
python3 tests/test_history.py
python3 tests/test_catalog.py
python3 tests/test_guidelines.py
```

---

## Contributing & Adding New Rules

1. Create `crates/svlang-lint/src/rules/<category>/<rule_name>.rs` implementing a `Check::Model`, `Check::Syntax`, or `Check::Mixed` rule.
2. Register the rule with its metadata in `crates/svlang-lint/src/rules/<category>/mod.rs`.
3. Add paired test cases in `testdata/rules/<category>/<RULE_ID>/{bad.sv, good.sv, expect.json}`.
4. Regenerate docs with `python3 tools/codegen/gen_rule_docs.py` and verify with `cargo test` and `./tools/gates/rust_gate.sh`.

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for full development guidelines and pull request conventions.

---

## License

This project is licensed under the Apache License 2.0; see the [LICENSE](../../LICENSE) and [NOTICE](../../NOTICE) files.

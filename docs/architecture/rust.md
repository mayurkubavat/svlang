# Architecture of the Rust workspace

This is the map of the Rust workspace (`Cargo.toml` at the repository root,
crates in `crates/`): what each crate does, how a SystemVerilog compile flows
through them, and where to start when changing something.
[rust-history.md](rust-history.md) has the build commands, tool usage and the history
of each stage with its measurements; `cargo doc --workspace --no-deps --open`
has the API of every crate.

## The pipeline

```
compile options / file lists (-f, +incdir+, +define+, -top, ...)
        │
        ▼
  sv-source ── FileList: sources, include dirs, defines, tops, problems
        │
        ▼
  sv-lexer ─── lossless tokens (trivia, directives and macro punctuation included)
        │
        ▼
  sv-preprocessor ─ `define / `ifdef / `include expanded; every token keeps where it came from
        │
        ▼
  sv-syntax ── lossless syntax tree per file (or one tree per unit)
        │
        ▼
  sv-semantic ──┬─ pure SV class model ─► svlang::Design ──► svlang-model::Design
            ├─ elaboration (instances, generate, parameter values, ElaboratedDesign)
            ├─ typed RTL expressions (ExprBinder) and bound SVA properties (SvaBinder)
            ├─ semantic checks (names, types, constants, classes, SVA, UDPs)
            └─ type oracle, specializations, locations (for rules)
        │
        ▼
  sv-netlist ─── word-level RTL netlist IR, procedural execution, driver checks, SVA lowering
        │
        ▼
  svlang ─────── single pure-Rust IEEE 1800 SDK (Driver, Compilation, re-exports)
        │
        ▼
  svlang-model ─► svlang-lint ─► svlang-lsp ─► svlang-cli (single `svlang` binary)
```

Everything in the 16 publishable crates (`sv-source` through `svlang-cli`) is
pure IEEE 1800-2023 Rust with zero C++ dependencies. Optional dev-only Slang
compiler dumpers (`tools/oracles/`, `cmake --preset oracles`) are strictly
external CLI oracles for differential compiler testing; see
`third_party/slang/README.md`.

## Crates

| Crate | Kind | Role |
| --- | --- | --- |
| `sv-source` | lib | Reads VCS/Xcelium file lists and compile options into a `FileList`, keeping the origin of every item and reporting problems instead of stopping. |
| `sv-lexer` | lib | Lossless lexer: every byte is in exactly one token, so tokens give the file back. Token set follows slang's so token streams compare one to one. |
| `sv-preprocessor` | lib | IEEE 1800-2023 clause 22 preprocessor. Output is the parser's token stream with no trivia or directives; each token records its file offset, the place its text was written, and whether a macro made it. |
| `sv-syntax` | lib | Event-based recovering parser. A `Tree` is a token array plus a node arena (`SyntaxKind` per node); trivia rides on tokens, so `Tree::file_text` rebuilds each file byte for byte. |
| `sv-bigint` | lib | Arbitrary-width two's complement integer (`Int`) for constant values wider than 128 bits; no dependencies. `sv-semantic` re-exports it as `sv_semantic::bigint`. |
| `sv-fxhash` | lib | The Fx hasher with `FxMap`/`FxSet` aliases for internal maps keyed by node ids and names; no dependencies. Re-exported as `sv_semantic::fx`, which `sv-netlist` and `svlang-lint` use. |
| `sv-semantic` | lib | Pure IEEE 1800-2023 semantic analysis: scopes and name resolution, constant evaluation (arbitrary width, four-state), elaborated design objects (`ElaboratedDesign`), IEEE 11.6/11.8 bound typed expressions (`ExprBinder`), bound SVA property/sequence trees (`SvaBinder`), type strings, the OOP class model, and compiler checks. |
| `sv-netlist` | lib | Format-neutral word-level RTL netlist IR (`Netlist`): procedural execution (`always_ff`, async reset metadata, `always_comb`, `always_latch`, `assign`, port stitching, SCC combinational loop detection), bit-level driver resolution, static loop unrolling, subroutine inlining, memory ports, and `blast_memories`. |
| `sv-btor2` / `sv-aiger` / `sv-yosys` | lib | Back-end writers emitting Btor2, bit-blasted AIGER, and Yosys word-level JSON from `sv-netlist`. |
| `svlang` | lib | Public umbrella SDK re-exporting `source`, `lexer`, `preprocessor`, `syntax`, `semantic`, `netlist`, `btor2`, `aiger`, and `yosys`, plus top-level `Driver` and `Compilation` APIs. |
| `svlang-model` | lib | Owned UVM/RAL design model: classes, UVM roles (`UvmKind`), UVM phases (`PhaseKind`), properties, methods, body facts, and RAL register maps (`ral`). |
| `svlang-lint` | lib | Static analysis and auto-fix engine: 446+ rules run in parallel over `svlang-model` and `sv-syntax` trees, with waivers, baselines, SARIF/HTML/JSON reports, and Python config/rule support. |
| `svlang-lsp` | lib | Language Server Protocol engine: code browsing (symbols, definition, references, hover, completion) on `svlang::syntax`, and live lint diagnostics with quick fixes from `svlang-lint`. |
| `svlang-cli` | bin (`svlang`) | The single unified CLI: `check`, `lint`, `lsp`, `filelist`, `map`, `preprocess`, `hier`, `cst`, `netlist`, `btor2`, `aiger`, `yosys`, and `stages` (plus `argv[0]` dispatch when symlinked as `svlang-lint`). |

Dependencies point strictly upward: `sv-lexer` ← `sv-preprocessor` ← `sv-syntax` ←
`sv-semantic` ← `sv-netlist` ← `svlang` ← `svlang-model` ← `svlang-lint` ← `svlang-lsp` ← `svlang-cli`.
`sv-semantic` also builds on crates split out of it, which depend on nothing
above them and which it re-exports at their old paths: `sv-bigint` and `sv-fxhash`.

## The front end in more detail

**Positions.** A preprocessed token carries an `Origin` (file index and
byte offset). For a token a macro produced, `origin` is the macro use and
`written` is where its text was written (in the `define body or a macro
argument). `sv_semantic::Locator` turns these into `svlang_model::Loc` (1-based
line and column) with one file table per design, and records the
expansion site, which is where svlang-lint reports findings on macro code.

**Compilation units.** `sv_syntax::parse_files` gives one tree per file
with macros shared across files, as simulators compile a file list.
`parse_unit` (`--single-unit`) gives one tree for all files.

**sv-semantic's builder.** A private `Builder` walks every tree once to
collect declarations into scopes (`scopes`), then resolves names on demand
with constant evaluation (`consteval`, `packed`, and `bigint` from the
`sv-bigint` crate). From it come:

- `build` / `load`: the class model, including type strings as slang
  prints them (`types`) and method body facts (`body`);
- `elaborate_full`: the instance tree from the top modules (`elab`);
- `SemaChecks`: one module per family of checks (`names`, `checks`,
  `bind`, `iclass`, `classes`, `order`, `calls`, `typed`, `sva`, `udp`),
  run on several threads and merged by `semantic_diags`;
- `oracle::TypeOracle`, `SpecModel`, `spec_counts`: what rules ask beyond
  the model (the declared type of a name, a class seen through each
  specialization, how many specializations slang would elaborate).

Each check's messages follow slang's wording where slang has the same
check, so `svlang check`'s output can be compared with slang's.

## The linter

`svlang_lint::rules()` lists every rule with a `Check`:

- `Model`: reads only `svlang_model::Design`, so it runs on either front end;
- `Syntax`: reads one `syntax::Cst` (a tree with parent links, per-kind
  node lists and a locator), run once per tree;
- `Mixed`: reads the model and every tree of the compile at once.

`lint_all` runs them on `SVLANG_JOBS` threads (default: all cores; `DV_LINT_JOBS`
also accepted). Each thread takes the next rule, with the slowest rules first. Work many rules
share (expression types in `expr_types`, sv-semantic's checks, elaboration,
specialization counts) lives in `shared`: built once per compile by the
first thread that asks, while the others wait. Findings are merged back in
the serial order and sorted by position and rule, then inline waivers are
applied, so output is the same for any thread count. `SVLANG_PROFILE=1`
prints the cost of each rule.

Every ported rule matches its C++ twin under `cpp/src/rules/` with the same id,
name, severity and message text; `tools/gates/lint_parity.py` compares the two
finding for finding.

The source is laid out by rule family, as `cpp/src/rules/` is:

```
crates/svlang-lint/src/
  lib.rs             Rule, Check, lint_all and the parallel runner
  rules/mod.rs       collects every family
  rules/<family>/    one file per rule, named after it, plus mod.rs that
                     registers the family (uvm/, methodology/, random/, ...)
  support/           helpers several rules share, one module per topic
                     (class_index, statements, slang_ast, ...)
  syntax.rs          Cst: a tree with parents, per-kind node lists, locations
  expr_types.rs      expression types as slang gives them (--exprs)
  shared.rs          work built once per compile and shared between threads
  session.rs         Session: the settings one run of the rules reads
  uvm_api.rs, waivers.rs
```

A rule file such as `rules/uvm/blocking_in_observer.rs` opens with the
rule's id, name and what it flags, and holds the helpers only that rule
uses.

The settings a run reads (rule options, the `style` and `naming` blocks,
`-top`, `--skip-libraries`) live in a `svlang_lint::Session`. A run hands them
to the threads it starts, so two sessions with different settings can lint
at the same time in one process: an editor with two workspaces, a test
harness, a host language. The command line uses the process's own session,
which is what the free functions (`svlang_lint::options::set`,
`svlang_lint::lint_all`) work on.

## The language server

`svlang lsp` (`crates/svlang-lsp`) indexes every `.sv/.svh/.v/.vh` file of the
workspace on its own (`index`: declarations with scopes, types and doc comments,
plus every identifier use), keeps a name table over all files (`workspace`), and
answers requests over stdio (`server`). Includes are not followed, so positions
always stay in the file that wrote them. `json` and `text` are its small JSON
and position/URI helpers; it needs no crates outside the workspace.

Diagnostics come from `svlang-lint` (`lint`). The editor's text of each open
file is set as an overlay (`sv_preprocessor::overlay`), which the preprocessor
and the waiver code read in place of the file. Every edit asks for a check;
a thread of its own waits for typing to pause, checks the project through
`svlang_lint::Workspace` (the project's config, waivers and baseline, the rule
selection of the command line) and publishes each open file's diagnostics.
What it published is kept, and code actions (a rule's fix, a waiver comment)
are answered from it. The project is reloaded when its config or waiver
files change.

## How correctness is checked

Each layer has a gate against slang, run from `tools/gates/` (most need
`UVM_SRC` pointing at a UVM source tree):

| Gate | Compares |
| --- | --- |
| `pp_parity.py` | preprocessed tokens |
| `cst_parity.py` | parse outline and canonical expressions (`--exprs`) |
| `model_parity.py` | `Design::dump` of the native and slang models (`--bodies`, `--types`) |
| `sym_gate.py` | scopes, types, values and hierarchy from `svlang check` |
| `ieee_gate.py` | errors on every sv-tests file (legal and should-fail) |
| `slang_unit_gate.py` | semantic errors on slang's own unit test cases |
| `expr_gate.py` | expression types from `svlang lint --exprs` |
| `spec_gate.py` | specialization counts from `svlang lint --specs` |
| `netlist_gate.py` | word-level RTL lowering, memory blasting, and fail-closed diagnostics from `svlang check --netlist` |
| `lint_parity.py` | `svlang lint` findings against C++ `svlang-lint` |

Plus `cargo test` (native) and `cargo test -p sv-slang`
(the Slang bridge and the tests that load through Slang), and
`rust_gate.sh`: `cargo fmt --check` (`rustfmt.toml`) and Clippy with
`-D warnings` on all targets, natively and on `sv-slang` when
`libsvlang_lint_model` is built. Clippy takes the MSRV from `rust-version` in
the root `Cargo.toml`. A change to the front end should keep
every gate where it was; a change to a rule should keep `lint_parity.py`
identical.

## Where to start

- A new or changed lint rule: open `rules/<family>/<rule_name>.rs`, or add
  a file there and register it in that family's `mod.rs` with a `Check` of
  the narrowest kind that works. Reuse
  `syntax::Cst` helpers and the caches in `shared` rather than rebuilding
  scopes or types in the rule.
- A parse problem: `sv-syntax` (`grammar`, `expr`, `blocks`, `assertions`,
  `misc`); check with `cst_parity.py`.
- A wrong type, value or error: `sv-semantic`, starting from the module whose
  check reports it; check with `sym_gate.py` and `ieee_gate.py`.
- An RTL lowering, procedural execution, driver, or SVA netlist issue:
  `sv-netlist` (`lower`, `proc`, `loops_sub`, `expr_lower`, `drivers`, `builder`,
  `ir`); check with `cargo test -p sv-netlist` and `netlist_gate.py`.
- A file list option: `sv-source`.

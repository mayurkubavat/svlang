# Contributing to svlang

Thank you for your interest in contributing to **svlang**! Bug reports, compiler and netlist improvements, IDE/LSP enhancements, and new verification/RTL lint rules are all welcome.

---

## Code of Conduct

Please be respectful, constructive, and collaborative in all issues, pull requests, and discussions.

---

## Development Environment Setup

### Prerequisites
- **Rust:** Rust `>= 1.82` (the workspace MSRV in `Cargo.toml`) with `cargo`, `rustfmt`, and `clippy`
- **Python:** Python `>= 3.9` (for end-to-end CLI, catalog, fix-invariant, and netlist gates)
- **Optional (dev-only compiler oracles):** C++20 compiler, CMake `>= 3.28`, and Ninja — only needed when running differential Slang compiler gates via `cmake --preset oracles`

### Building and Testing
```bash
git clone https://github.com/mayurkubavat/svlang.git
cd svlang

# Build the unified `svlang` CLI and run all Rust unit/integration/doc tests
cargo build --release -p svlang-cli
cargo test

# Run formatting, Clippy (-D warnings), and guideline consistency checks
./tools/gates/rust_gate.sh
./tools/gates/hygiene.sh

# Run end-to-end CLI, fix-invariant, and netlist gates
python3 tools/gates/netlist_gate.py
python3 tests/test_fix_invariants.py
```

The workspace crates (`crates/sv-*`, `crates/svlang`, `crates/svlang-model`, `crates/svlang-lint`, `crates/svlang-lsp`, `crates/svlang-cli`) are pure Rust. The core compiler/netlist SDK (`svlang`) has zero external crate dependencies; `svlang-lint` uses `regex` (with Unicode/SIMD features disabled) behind an ECMAScript compatibility gate (`tools/gates/regex_gate.py`).

### Repository Hygiene

Assistant and agent state (`.claude/`, `CLAUDE.md`, `.mcp.json`, etc.), keys, and credentials stay out of git. `tools/gates/hygiene.sh` fails if any are tracked, or if a tracked file or unpushed commit carries assistant attribution trailers. Enable the pre-commit/commit-msg hook once per clone:

```sh
git config core.hooksPath tools/hooks
```

---

## Rule Taxonomy & Prefix Allocation (`DOM-GRP-NNN`)

Every rule and sub-rule in `svlang-lint` **must** follow the strict 2-tier **`DOM-GRP-NNN`** (`3-3-3`) naming scheme:
- **`DOM`**: 3-letter uppercase domain (`CMP`, `RTL`, `SVL`, `SVA`, `UVM`)
- **`GRP`**: 3-letter uppercase group from the normative table below
- **`NNN`**: 3-digit zero-padded sequence number (`001`..`999`; `100` / `101+` reserved for configurable multi-target engines)

`crates/svlang-lint/tests/catalog.rs` enforces both the `^(CMP|RTL|SVL|SVA|UVM)-[A-Z]{3}-[0-9]{3}$` format and the deterministic `Prefix -> Category` mapping below:

| Prefix | Domain | Required Category | Implementation Folder(s) (`crates/svlang-lint/src/rules/`) | Scope |
| :--- | :--- | :--- | :--- | :--- |
| `CMP-ERR` | `CMP` | `Compiler` | `compiler/` | Preprocessor, syntax, elaboration, and semantic compiler diagnostics |
| `RTL-SYN` | `RTL` | `RTL` | `correctness/`, `concurrency/`, `analysis/` | Synthesizable sequential/comb blocks, resets, gated clocks, latches, clocked races |
| `RTL-FSM` | `RTL` | `RTL` | `correctness/` | FSM enum case exhaustiveness, unreachable states, and deadlock states |
| `RTL-CDC` | `RTL` | `RTL` | `correctness/` | Unsynchronized clock-domain crossings |
| `RTL-DAT` | `RTL` | `RTL` | `correctness/` | Case defaults, `casex`/X-equality hazards, reversed part-selects, concat widths |
| `SVL-COR` | `SVL` | `Correctness` | `correctness/`, `errors/` | General SV correctness, branch/copy bugs, subroutine CFG dataflow, error handling, timescales |
| `SVL-TYP` | `SVL` | `Types` | `types/`, `methodology/` | Signed/unsigned mixing, enum/array/struct casts & patterns, loop counters |
| `SVL-OOP` | `SVL` | `OOP` | `oop/`, `design/` | Virtual dispatch, `$cast`, constructors, shallow copy, shadowing, SOLID contracts |
| `SVL-CON` | `SVL` | `Concurrency` | `concurrency/` | `fork`/`join` captures, event races, semaphores/mailboxes, clocking blocks, VIF drives |
| `SVL-RND` | `SVL` | `Randomization` | `random/`, `analysis/`, `methodology/` | `randomize()`, contradictory/soft/dist constraints, solver blow-up, constant reseeding |
| `SVL-COV` | `SVL` | `Coverage` | `coverage/`, `analysis/` | Covergroups, cross-bin explosion, bin overlaps, `per_instance` options |
| `SVL-PRF` | `SVL` | `Performance` | `performance/` | By-value array copies, `O(N^2)` queue/string loops, `#0` delays, loop allocations |
| `SVL-DED` | `SVL` | `DeadCode` | `deadcode/` | Unused locals, args, params, typedefs, macros, fields, dead stores, unreachable code |
| `SVL-MNT` | `SVL` | `Maintainability` | `metrics/`, `design/`, `smells/`, `readability/`, `methodology/` | Complexity metrics, design/package smells, clones, readability, git-history hotspots |
| `SVL-WVR` | `SVL` | `Maintainability` | `style/`, `waivers.rs` | Unclosed block waivers and invalid/unused waiver hygiene diagnostics |
| `SVL-PRT` | `SVL` | `Portability` | `portability/` | Proprietary vendor system tasks, host paths, `$unit` pollution, header macro leaks |
| `SVL-DPI` | `SVL` | `Portability` | `dpi/`, `analysis/` | Conflicting DPI-C C prototypes and non-portable 4-state DPI types |
| `SVL-SEC` | `SVL` | `Security` | `security/` | Plaintext secret logging and unchecked `$fopen` descriptors |
| `SVL-DEP` | `SVL` | `Deprecated` | `deprecated/` | Deprecated `program`/`defparam`/`always`/`$random`/`assign`/non-ANSI & legacy `reg` |
| `SVL-STY` | `SVL` | `Style` | `style/`, `methodology/` | End labels, whitespace, line length, banners, include guards, configurable style engine |
| `SVL-NAM` | `SVL` | `Naming` | `style/` | Package, interface, typedef, macro, constraint, and configurable SV naming conventions |
| `SVA-PRP` | `SVA` | `SVA` | `sva/`, `readability/`, `methodology/` | SVA clocks, resets, action blocks, vacuous antecedents, unbounded delays, interface contracts |
| `SVA-FPV` | `SVA` | `SVA` | `sva/` | Pre-FPV formal-readiness checks (`--preset formal-ready`) |
| `UVM-FAC` | `UVM` | `UVM` | `uvm/`, `methodology/` | Factory registration macros, parameterized utils, overrides, parent handles |
| `UVM-PHS` | `UVM` | `UVM` | `uvm/`, `methodology/` | Phase signatures, `super.<phase>` order, zero-time function phases, runtime domains |
| `UVM-OBJ` | `UVM` | `UVM` | `uvm/`, `errors/` | Inter-procedural objection CFG pairing, loop/fork objection hazards, drain delays |
| `UVM-CFG` | `UVM` | `UVM` | `uvm/`, `methodology/` | `uvm_config_db` type/path resolution, wildcard cost, runtime misuse, config objects |
| `UVM-SEQ` | `UVM` | `UVM` | `uvm/`, `methodology/` | `start_item`/`finish_item`, sequence/sequencer typing, `p_sequencer`, virtual sequencers |
| `UVM-TLM` | `UVM` | `UVM` | `uvm/`, `analysis/`, `errors/` | TLM port/FIFO connectivity, analysis transaction mutation, FIFO starvation, scoreboards |
| `UVM-CMP` | `UVM` | `UVM` | `uvm/`, `design/`, `oop/`, `errors/`, `methodology/` | Component layering, Law of Demeter, callbacks, UVM reporting, roles, passive VIF drives |
| `UVM-RAL` | `UVM` | `UVM` | `ral/`, `uvm/`, `methodology/` | Register block locking, address/field overlaps, orphan regs, adapter/predictor wiring |
| `UVM-DEP` | `UVM` | `Deprecated` | `deprecated/`, `uvm/` | Deprecated UVM 1.1d/1.2/1800.2 APIs, `starting_phase`, `uvm_field_*`, `uvm_do*` |
| `UVM-PRT` | `UVM` | `Portability` | `uvm/` | Non-IEEE `@uvm-accellera` and `@uvm-contrib` UVM APIs |
| `UVM-NAM` | `UVM` | `Naming` | `style/` | UVM component, sequence, item, TLM port, `create()` name, and member prefix conventions |

---

## Adding a New Rule

### 1. Implement and Register the Rule
1. Pick the next sequential `NNN` under the appropriate `DOM-GRP` prefix and create `crates/svlang-lint/src/rules/<family>/<rule_name>.rs`:
   - `Check::Model` — reads only `svlang_model::Design`.
   - `Check::Syntax` — walks `syntax::Cst` concrete syntax trees.
   - `Check::Mixed` — combines `Design` and `&[Cst]`.
   - Reuse shared semantic helpers in `crates/svlang-lint/src/support/` (`class_index`, `class_roles`, `cfg_dataflow`, `constraint_ir`, `rtl_fsm_cdc`, `formal_ready`, `layout`) and `crate::options::{get, get_str, get_list}`.
   - Put house-style rules in `profile::STYLE` and opinionated best-practice suggestions in `profile::ADVISORY` so `DEFAULT_PROFILES` remains high-signal on real RTL and UVM repositories.
2. Register the `Rule` in `crates/svlang-lint/src/rules/<family>/mod.rs` with `native: true`, the prefix's required `category`, and a complete sentence in `about` (ending with `.`).
3. If the rule enforces a [UVM Methodology Guide](docs/methodology/README.md) entry, add `"MG-x.y"` to `guidelines` and run `python3 tools/codegen/gen_guidelines.py`.

### 2. Add Paired `bad.sv` / `good.sv` Fixtures
1. Create `testdata/rules/<area>/<DOM-GRP-NNN>/` with:
   - `bad.sv` — minimal, self-contained code triggering the finding.
   - `good.sv` — compliant counterpart producing zero findings for the rule.
   - `expect.json` — expected finding lines in `bad.sv`:
     ```json
     { "bad.sv": [ { "rule": "RTL-FSM-004", "line": 17 } ] }
     ```

### 3. Regenerate the Rule Catalog & Run Gates
```bash
cargo build --release -p svlang-cli
python3 tools/codegen/gen_rule_docs.py
python3 tools/codegen/gen_rule_docs.py --check

cargo test -p svlang-lint
python3 tests/test_catalog.py
python3 tests/test_explain.py
python3 tests/test_guidelines.py
python3 tests/test_fix_invariants.py   # required when adding or modifying --fix rewrites
```

---

## Optional Dev-Only Slang Compiler Oracles

Compiler front-end changes (`crates/sv-preprocessor`, `crates/sv-syntax`, `crates/sv-checks`, `crates/sv-expr`, `crates/sv-semantic`) can be cross-checked against upstream Slang using the standalone dumpers in `tools/oracles/` (keep CMake build parallelism at `-j 4`):

```bash
cmake --preset oracles && cmake --build build -j 4
./tools/gates/prepush.sh --oracles
```

---

## Pull Request Checklist

1. `cargo test`, `./tools/gates/rust_gate.sh`, and `./tools/gates/hygiene.sh` pass.
2. `python3 tools/codegen/gen_rule_docs.py --check` and `python3 tools/codegen/gen_guidelines.py --check` pass (never hand-edit `docs/rules/README.md` or `docs/rules/catalog.json`).
3. Relevant Python gates pass (`tests/test_*.py`, `tools/gates/netlist_gate.py`, and `tools/quality/error_budget.py` when `$UVM_HOME` is available).
4. Safe `--fix` rewrites preserve simulation semantics and pass `python3 tests/test_fix_invariants.py`.

## Licensing & Security

By submitting a contribution you agree that it is licensed under the project's [LICENSE](LICENSE); third-party notices belong in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). For security vulnerabilities, please follow [SECURITY.md](SECURITY.md) rather than opening a public issue.

# Contributing to svlang

Thank you for your interest in contributing to **svlang**! We welcome bug reports, feature suggestions, compiler and netlist improvements, and new verification rules.

---

## Code of Conduct

Please be respectful, constructive, and collaborative in all issues, pull requests, and discussions.

---

## Development Environment Setup

### Prerequisites
- **Rust:** Rust `>= 1.82` (the workspace MSRV in `Cargo.toml`) with `cargo`, `rustfmt`, and `clippy`
- **Python:** Python `>= 3.9` (for end-to-end CLI, fix-invariant, and netlist gates)
- **Optional (dev-only compiler oracles):** C++20 compiler, CMake `>= 3.28`, and Ninja — only needed if running differential compiler gates via `cmake --preset oracles`

### Building and Testing
```bash
git clone https://github.com/mayurkubavat/svlang.git
cd svlang

# Build the unified `svlang` CLI and run all Rust unit/integration/doc tests
cargo build --release -p svlang-cli
cargo test

# Run formatting, Clippy (-D warnings), and guideline consistency checks
./tools/gates/rust_gate.sh

# Run end-to-end CLI, fix-invariant, and netlist gates
python3 tools/gates/netlist_gate.py
python3 tests/test_fix_invariants.py
```

The workspace crates (`crates/sv-*`, `crates/svlang`, `crates/svlang-model`, `crates/svlang-lint`, `crates/svlang-lsp`, `crates/svlang-cli`) are pure Rust. The core compiler/netlist SDK (`svlang`) has zero external crate dependencies; `svlang-lint` uses `regex` (with Unicode/SIMD features disabled) behind an ECMAScript compatibility check (`tools/gates/regex_gate.py`).

---

### Repository hygiene

Assistant and agent state (`.claude/`, `CLAUDE.md`, `.mcp.json` and the like), keys and credentials stay out of git; `.gitignore` covers them. `tools/gates/hygiene.sh` fails if any are tracked, or if a tracked file or an unpushed commit carries assistant attribution (co-author trailers and similar). CI and `tools/gates/prepush.sh` run it. To catch an attribution line at commit time, enable the hook once per clone:

```sh
git config core.hooksPath tools/hooks
```

---

## Adding a New Verification Rule

### 1. Choose Rule Category & ID
Locate the relevant category folder under `crates/svlang-lint/src/rules/`:
- `analysis/` - Cross-design analysis (covergroup sampling, uncovered analysis ports)
- `compiler/` - Compiler diagnostics (`SV-CMP-*`)
- `concurrency/` - Race conditions, clocking blocks, fork-join pitfalls
- `correctness/` - Timescales, bitwidth mismatches, off-by-one errors
- `coverage/` - Covergroup syntax, bin limits, illegal bins
- `deadcode/` - Unreachable code, unused locals/fields/parameters
- `deprecated/` - Legacy OVM / banned UVM APIs
- `design/` - Testbench layers, hierarchy reach-through, god class, cohesion, coupling, overrides
- `duplication/` - Copy-paste code and repeated argument groups
- `errors/` - Unreported failures, checkers that cannot fail, use after failed check
- `evolution/` - Git-history hotspots and change coupling
- `formal/` - Pre-FPV formal-readiness checks (`SV-FPV-*`)
- `methodology/` - Rules enforcing the [UVM Methodology Guide](docs/methodology/README.md)
- `metrics/` - Cognitive complexity, routine length, nesting, argument count, class size
- `oop/` - Virtual methods, downcasting, constructors
- `performance/` - Dynamic array copies, string bottlenecks
- `ral/` - Register map, address collisions, adapter/predictor wiring
- `random/` - Unchecked `randomize()`, constraint solving issues
- `readability/` - Flag arguments, const ref, commented-out code, magic numbers
- `rtl/` - Synthesizable RTL, FSM, CDC, and dataflow rules
- `style/` - End labels, line length, formatting, naming conventions
- `sva/` - Vacuous assertions, combinatorial triggers, reset handling
- `types/` - Mixed-sign comparisons, redundant `$cast`, DPI prototypes
- `uvm/` - Factory, phasing, objections, `uvm_config_db`, sequences, TLM topology

Shared analysis helpers live in `crates/svlang-lint/src/support/` (class indices, UVM roles, CFG/dataflow, statement/call walkers, expression types, routine/class metrics). Rule options go through `crate::options` so project configs can override thresholds per rule. Opinionated rules belong to the `Advisory` profile (`profile::ADVISORY`), so they stay off unless `--preset advisory`, `--preset all`, or `--rule` selects them.

Assign a unique rule ID conforming to the standard naming scheme (e.g., `UVM-XYZ-001`, `SV-PRF-013`).

### 2. Implement and Register the Rule
1. Create `crates/svlang-lint/src/rules/<category>/<rule_name>.rs`.
   - Use `Check::Model` for rules that read only `svlang_model::Design`.
   - Use `Check::Syntax` for rules that walk `syntax::Cst` concrete syntax trees.
   - Use `Check::Mixed` for rules that combine both.
   - Attach related locations (`Finding::related`) and precise source spans (`Finding::range`) when a finding involves a second declaration or call site. They flow automatically to console `note:` lines, JSON, SARIF, HTML, and the LSP server.
2. Register the rule with its ID, name, category, severity, profiles, guidelines, maturity, and description in `crates/svlang-lint/src/rules/<category>/mod.rs`.
3. If the rule enforces a guideline in the [UVM Methodology Guide](docs/methodology/README.md), include that guideline ID in the rule's `guidelines` slice and regenerate the guideline table (`python3 tools/codegen/gen_guidelines.py`).

### 3. Regenerate the Rule Catalog
The published catalog (`docs/rules/catalog.json`, `docs/rules/README.md`, and the `Enforced by:` lines in `docs/methodology/`) is generated directly from the rule metadata in `svlang-lint`:

```bash
cargo build -p svlang-cli --bin svlang
python3 tools/codegen/gen_rule_docs.py          # updates docs/rules/{catalog.json,README.md} & docs/methodology/
python3 tools/codegen/gen_rule_docs.py --check  # what CI runs: fails if any generated doc is stale
```

### 4. Create Paired Unit Tests
Every rule **must** ship with paired `bad.sv` and `good.sv` cases plus `expect.json`:
1. Create `testdata/rules/<category>/<RULE_ID>/`.
2. Add `bad.sv` containing an isolated violation that triggers the rule.
3. Add `good.sv` containing the compliant implementation that passes cleanly.
4. Add `expect.json` listing every finding `bad.sv` must produce by rule and line:
   ```json
   { "bad.sv": [ { "rule": "UVM-XYZ-001", "line": 17 } ] }
   ```
5. Verify with Rust and Python suites:
   ```bash
   cargo test -p svlang-lint -- rules
   python3 tests/test_catalog.py
   python3 tests/test_guidelines.py
   python3 tests/test_fix_invariants.py   # required when adding or changing --fix rewrites
   ```

---

## Optional Dev-Only Slang Compiler Oracles

Compiler front-end changes (`crates/sv-preprocessor`, `crates/sv-syntax`, `crates/sv-semantic`) can be cross-checked against upstream Slang using the optional standalone dumpers in `tools/oracles/` (keep build parallelism at `-j 4`):

```bash
cmake --preset oracles && cmake --build build -j 4
./tools/gates/prepush.sh --oracles
```

These oracles are developer-only and are not required to build, test, or ship `svlang`.

---

## Pull Request Guidelines

1. **Clean Gates**: `cargo test`, `./tools/gates/rust_gate.sh`, and `./tools/gates/hygiene.sh` must pass.
2. **End-to-End Suites**: Run `python3 tools/gates/netlist_gate.py` for compiler/netlist changes and the relevant `tests/test_*.py` suites for linter/CLI changes.
3. **Generated Docs**: Run `python3 tools/codegen/gen_rule_docs.py` after adding a rule or changing its metadata; never hand-edit [docs/rules/README.md](docs/rules/README.md) or [docs/rules/catalog.json](docs/rules/catalog.json).
4. **Automated Fixes**: Safe `--fix` rewrites must preserve simulation behaviour and pass `python3 tests/test_fix_invariants.py`.

## Licensing of contributions

By submitting a contribution you agree that it is licensed under the project's
[LICENSE](LICENSE), and that you have the right to submit it. Code copied from
another project must keep its license notice and be listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Reporting security issues

Please do not open public issues for vulnerabilities; see [SECURITY.md](SECURITY.md).

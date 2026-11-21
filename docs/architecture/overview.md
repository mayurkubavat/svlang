# Architecture overview

`svlang` is a 100% pure-Rust IEEE 1800-2023 SystemVerilog & UVM compiler,
static linter (`svlang lint`), language server (`svlang lsp`), and RTL/formal
lowering toolchain in one binary (`crates/`). [rust.md](rust.md) goes deeper
into the Rust workspace.

## Architecture

```
  sv-source -> sv-lexer -> sv-preprocessor -> sv-syntax -> sv-semantic -> sv-netlist
                                                               |               |
                                                               |               +-> sv-btor2 / sv-aiger / sv-yosys
                                                               v
                                                            svlang  (Rust SDK + pysvlang + svlang-wasm)
                                                               |
                                                               v
                                                         svlang-model -> svlang-lint -> svlang-lsp -> svlang-cli (`svlang`)
```

- **`svlang`** (`crates/svlang-cli`) is the single unified binary:
  `svlang check`, `svlang lint`, `svlang lsp`, `svlang filelist`, `svlang map`,
  `svlang preprocess`, `svlang hier`, `svlang cst`, `svlang netlist`,
  `svlang btor2`, `svlang aiger`, `svlang yosys`, and `svlang stages`.
- **`python/svlang_lint/`** is the Python configuration and custom-rule SDK
  (invoked by `svlang lint` for `.svlang-lint/config.py` and `--python-rules`).
- **`tools/oracles/`** (~880 LOC of standalone C++ Slang dumpers) is an
  optional dev-only differential compiler reference (`cmake --preset oracles`)
  used by compiler conformance gates in `tools/gates/`. Nothing in CI,
  releases, or `cargo build`/`cargo test` builds or requires C++.

## Verification & Gates

- `cargo test` runs unit, integration, and rule fixture suites (`testdata/rules/`).
- `tests/test_*.py` and `python/tests/test_python_rules.py` run end-to-end CLI,
  catalog, waiver, baseline, `--init`, `--map`, `--explain`, and `--fix`
  mutation/idempotency invariant suites against `target/release/svlang`.
- `tools/gates/rust_gate.sh` enforces guideline freshness, `cargo fmt --check`,
  and `cargo clippy --all-targets -- -D warnings`.
- `tools/gates/netlist_gate.py` checks RTL lowering determinism and fail-closed diagnostics.
- Optional Slang differential gates (`pp_parity.py`, `cst_parity.py`,
  `sym_gate.py`, `ieee_gate.py`, `slang_unit_gate.py`, `expr_gate.py`,
  `spec_gate.py`, `regex_gate.py`) compare `svlang` against `tools/oracles/`.

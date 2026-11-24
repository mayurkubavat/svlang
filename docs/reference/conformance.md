# IEEE 1800 and 1800.2 Conformance

`svlang` compiles IEEE 1800-2023 SystemVerilog and IEEE 1800.2 UVM in pure Rust, and cross-checks its preprocessor, parser, elaboration, type system, and netlist lowering against suite gates and optional differential compiler oracles.

## LRM Clause Probes & Conformance Gates

- `testdata/conformance/lrm/` and `testdata/conformance/ieee1800_2023/` hold positive and negative IEEE 1800-2023 probes across clauses 5 through 35 (`testdata/conformance/clauses.json`).
- Legal probes must compile with zero `CMP-ERR-001` diagnostics under `svlang check` and `svlang lint --preset all --experimental`, while negative probes must be rejected cleanly.
- Clauses without compile-time static semantics are documented as `out_of_scope` in `clauses.json` (Clause 32 SDF back-annotation and Clause 34 encrypted envelopes).

```sh
cargo test -p svlang --test ieee_and_census_fixes
python3 tools/gates/netlist_gate.py
```

With the optional dev-only Slang dumpers built (`cmake --preset oracles && cmake --build build -j 4`), differential gates compare `svlang` against Slang across preprocessing, syntax trees, symbol binding, generic specializations, expression types/widths, and Slang's own AST unit tests:

```sh
python3 tools/gates/ieee_gate.py
python3 tools/gates/sym_gate.py --specs --types --values
python3 tools/gates/expr_gate.py
python3 tools/gates/slang_unit_gate.py
```

## UVM Release API Packs (`1.1d`, `1.2`, `2020.3.1`)

`svlang_lint::uvm_api` detects the UVM release being compiled from `uvm_pkg`'s version parameters and selects the matching API pack embedded from `crates/svlang-lint/data/uvm-api/`:

```sh
python3 tools/codegen/gen_uvm_api_pack.py ../uvm-core --out crates/svlang-lint/data/uvm-api
```

`SVLANG_LINT_UVM_API_PACK=<file>` overrides the embedded pack for a single run. Rules `UVM-PRT-001` (`AccelleraOnlyApi`), `UVM-PRT-002` (`ContribOnlyApi`), `UVM-DEP-008` (`DeprecatedUvmMacro`), and `UVM-DEP-009` (`DeprecatedUvmApi`) read directly from the active version pack, so supporting a new UVM release only requires generating its pack JSON.

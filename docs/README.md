# Documentation Index

## Editor & IDE Integration

- [VS Code, Cursor & Antigravity Extension (`svlang.svlang`)](../editors/vscode/README.md): Mode 1 live syntax/semantic checks, on-demand/on-save 475-rule linting, Status Bar Control Center, RTL Hierarchy, Signal Driver/Load Trace, and `.vscode/settings.json` recipes.

## Linter & CLI (`svlang`)

- [`svlang` CLI Overview](../crates/svlang-cli/README.md): unified `check`, `lint`, `lsp`, `filelist`, `map`, `hier`, `cst`, `netlist`, `btor2`, `aiger`, and `yosys` subcommands.
- [svlang-lint User Guide](guide/svlang-lint.md): presets, `DOM-GRP-NNN` prefix wildcards, safe/unsafe `--fix`, SARIF/JSON/HTML reports, baselines, inline/block waivers, LSP, pre-commit, and GitHub Actions.
- [Project Configuration](guide/project_config.md): `.svlang-lint/config.py`, layered configs, library paths, per-rule options, and expiring/re-grading waivers.
- [Python Rule SDK](guide/python_rule_sdk.md): authoring custom corporate or project rules in Python.
- [Incremental Result Cache](guide/result_cache.md): how `.svlang_lint_cache/` tracks compilation units, includes, and rule options.

## Rules & Methodology

- [Rule Catalog (`DOM-GRP-NNN`)](rules/README.md): generated catalog of all 475 rules and sub-rules across 5 domains (`CMP`, `RTL`, `SVL`, `SVA`, `UVM`), 34 prefixes, and 18 categories ([`catalog.json`](rules/catalog.json) for tools).
- [UVM Methodology Guide (`MG-1`..`MG-9`)](methodology/README.md): architectural guidelines for layered, reusable UVM verification environments and the rules enforcing each.
- [Deep Semantic & Dataflow Rules](rules/native-rules.md): constraint-interval solver, declaration-resolved virtual interface checks, and inter-procedural objection CFG pairing.

## Compiler & Netlist SDK (`svlang` / `pysvlang`)

- [`svlang` Rust SDK](../crates/svlang/README.md): `Driver` and `Compilation` APIs with runnable [`examples/`](../crates/svlang/examples/) and [docs.rs/svlang](https://docs.rs/svlang).
- [`pysvlang` Python Bindings](../crates/svlang-py/README.md): parsing, elaboration, hierarchy inspection, and netlist lowering from Python.
- [`sv-netlist` Word-Level RTL IR](../crates/sv-netlist/README.md): procedural lowering, SVA NFA/DFA synthesis, loop unrolling, black-boxing, and formal backends.

## Architecture & Conformance Reference

- [Workspace Overview](architecture/overview.md): repository map, pure-Rust crate stack, and optional differential compiler oracles.
- [Rust Architecture](architecture/rust.md): crate boundaries, parallel lint scheduler, LSP server, and conformance gates.
- [Rust Engineering History](architecture/rust-history.md): milestone measurements and architectural evolution.
- [IEEE 1800 & 1800.2 Conformance](reference/conformance.md): LRM clause probes, `ieee_gate.py`, and UVM version API packs.
- [sv-tests Conformance](reference/sv-tests.md): clause-by-clause results across 1,024 public SystemVerilog tests.
- [RTL Netlist Census](reference/census.md): open-source RTL elaboration and netlist lowering coverage.
- [Measured Vendor Tables](reference/vendor-tables.md): empirical simulator behavior on non-standard SystemVerilog constructs.
- [OpenTitan DV Precision Study](reference/precision-opentitan.md): zero-ERROR precision validation on `lowrisc_ip/dv`.
- [Optional Slang Oracles](../third_party/slang/README.md): dev-only `cmake --preset oracles` reference dumpers.

## Contributing

- [CONTRIBUTING.md](../CONTRIBUTING.md): `DOM-GRP-NNN` prefix allocation table, adding rules and `bad.sv`/`good.sv` fixtures, and verification gates.

# Documentation

## Using the svlang library

- [svlang crate](../crates/svlang/README.md): the Rust API, with examples;
  reference on [docs.rs](https://docs.rs/svlang).
- [pysvlang](../crates/svlang-py/README.md): the Python bindings.
- [sv-netlist](../crates/sv-netlist/README.md): the word-level RTL netlist.
- [Rust architecture](architecture/rust.md): how the crates fit together.

## Using the linter

- [svlang-lint guide](guide/svlang-lint.md): building, running, profiles, fixes,
  reports, Python rules, LSP, CI, baselines and waivers.
- [Project configuration](guide/project_config.md): `.svlang-lint/config.py`,
  library paths, per-rule options and waivers in Python.
- [Python rule SDK](guide/python_rule_sdk.md): writing rules in Python.
- [Result cache](guide/result_cache.md): how incremental runs reuse results.
- [VS Code extension](../editors/vscode/README.md).

## Rules and methodology

- [Rule catalog](rules/README.md) (generated; `rules/catalog.json` is the same as JSON).
- [UVM methodology guide](methodology/README.md): guidelines MG-1 to MG-9 and
  the rules that enforce them.

## Reference

- [IEEE 1800 conformance corpus](reference/conformance.md).
- [Measured vendor tables](reference/vendor-tables.md): which non-standard
  SystemVerilog each tool accepts.
- [sv-tests conformance](reference/sv-tests.md): svlang on the public
  SystemVerilog conformance suite, by clause.
- [Front-end census](reference/census.md): which constructs stop open RTL
  designs from lowering to the netlist, ranked by the modules they block.
- Robustness: `tools/quality/mutate_smoke.py` damages source files in small
  ways and checks that svlang reports errors and never crashes or hangs.
- [Semantic-analysis rules](rules/native-rules.md): constraint solver,
  declaration-resolved interface rules, and cross-routine objection pairing.
- [Optional Slang differential oracles](../third_party/slang/README.md): pin,
  patches and updating it for `tools/oracles/`.

## Architecture

- [Overview](architecture/overview.md): workspace layout, crates, and optional
  differential compiler oracles.
- [Rust workspace](architecture/rust.md): crates, pipeline, parallel rule
  engine, language server, gates.
- [Rust history](architecture/rust-history.md): build and tool usage, and the
  record of each stage (R0 bridge to N6) with its measurements.

## Contributing

- [CONTRIBUTING.md](../CONTRIBUTING.md): adding a rule, its tests and its docs.

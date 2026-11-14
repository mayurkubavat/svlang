# Documentation

## Using the svlang library

- [svlang crate](../crates/svlang/README.md): the Rust API, with examples;
  reference on [docs.rs](https://docs.rs/svlang).
- [pysvlang](../crates/svlang-py/README.md): the Python bindings.
- [sv-netlist](../crates/sv-netlist/README.md): the word-level RTL netlist.
- [Rust architecture](architecture/rust.md): how the crates fit together.

## Using the linter

- [uvmcheck guide](guide/uvmcheck.md): building, running, profiles, fixes,
  reports, Python rules, LSP, CI, baselines and waivers.
- [Project configuration](guide/project_config.md): `.uvmcheck/config.py`,
  library paths, per-rule options and waivers in Python.
- [Python rule SDK](guide/python_rule_sdk.md): writing rules in Python.
- [Result cache](guide/result_cache.md): how incremental runs reuse results.
- [VS Code extension](../editors/vscode/README.md).

## Rules and methodology

- [Rule catalog](rules/README.md) (generated; `rules/catalog.json` is the same as JSON).
- [UVM methodology guide](methodology/README.md): guidelines MG-1 to MG-9 and
  the rules that enforce them.

## Reference

- [Slang compatibility layer](reference/compat.md): the overlay that works
  around Slang limitations.
- [IEEE 1800 conformance corpus](reference/conformance.md).
- [Measured vendor tables](reference/vendor-tables.md): which non-standard
  SystemVerilog each tool accepts, and the slang options that match it.
- [sv-tests conformance](reference/sv-tests.md): svlang on the public
  SystemVerilog conformance suite, by clause.
- [Front-end census](reference/census.md): which constructs stop open RTL
  designs from lowering to the netlist, ranked by the modules they block.
- [Rules only dv-lint has](rules/native-rules.md).
- [Slang: pin, patches and updating it](../third_party/slang/README.md).

## Architecture

- [Overview](architecture/overview.md): the two engines, what they share, and
  how they are checked against each other.
- [Rust workspace](architecture/rust.md): crates, pipeline, parallel rule
  engine, language server, gates.
- [Rust history](architecture/rust-history.md): build and tool usage, and the
  record of each stage (R0 bridge to N6) with its measurements.

## Contributing

- [CONTRIBUTING.md](../CONTRIBUTING.md): adding a rule, its tests and its docs.

# svlang

[![crates.io](https://img.shields.io/crates/v/svlang.svg)](https://crates.io/crates/svlang)
[![docs.rs](https://img.shields.io/docsrs/svlang)](https://docs.rs/svlang)
[![PyPI](https://img.shields.io/pypi/v/pysvlang.svg)](https://pypi.org/project/pysvlang/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A SystemVerilog (IEEE 1800-2023) compiler front end written in pure Rust,
usable from Rust and from Python. It reads simulator filelists, preprocesses,
parses to a lossless syntax tree, elaborates the design hierarchy, runs the
compiler checks, and lowers synthesizable RTL and SVA to a word-level
netlist. There is no C++ toolchain to build and no dependency on any other
crate.

It is the library to reach for when you are building a linter, a language
server, a formatter, a documentation generator, a formal or equivalence
checker, or any script that needs to understand SystemVerilog.

```sh
cargo add svlang        # Rust
pip install pysvlang    # Python 3.9+, wheels for Linux, macOS and Windows
```

## What is in it

| Stage | Crate | What it does |
|---|---|---|
| Filelists | [`sv-source`](crates/sv-source/) | VCS and Xcelium `-f`/`-F` lists, `$VAR` expansion, `+incdir+`, `+define+`, `-G`, `-y`/`-v` |
| Lexer | [`sv-lexer`](crates/sv-lexer/) | lossless tokenizer: every source byte is kept |
| Preprocessor | [`sv-preprocessor`](crates/sv-preprocessor/) | Clause 22 macros, `` `include ``, `` `ifdef ``, with the origin of every token |
| Syntax | [`sv-syntax`](crates/sv-syntax/) | error-recovering lossless concrete syntax tree |
| Semantics | [`sv-semantic`](crates/sv-semantic/) | scopes, 4-state constant evaluation, elaboration, bound expressions and SVA, compiler checks, class model |
| Netlist | [`sv-netlist`](crates/sv-netlist/) | word-level RTL netlist: procedural blocks, driver analysis, loop unrolling, SVA lowering, memories, cut points and black boxes |
| Btor2 | [`sv-btor2`](crates/sv-btor2/) | the netlist as a Btor2 transition system for model checkers (rIC3, Pono, AVR, BtorMC) |
| AIGER | [`sv-aiger`](crates/sv-aiger/) | the netlist bit-blasted to an and-inverter graph in AIGER, for bit-level model checkers and ABC |
| Yosys | [`sv-yosys`](crates/sv-yosys/) | the netlist as a Yosys JSON netlist of word-level cells, for synthesis, equivalence and SymbiYosys |
| All of it | [`svlang`](crates/svlang/) | one crate that re-exports the stages behind `Driver` and `Compilation` |
| Command line | [`svlang-cli`](crates/svlang-cli/) | the unified `svlang` binary: `check`, `lint`, `lsp`, `filelist`, `map`, `hier`, `cst`, `netlist`, `btor2`, `aiger`, `yosys` |
| Model & Lint | [`svlang-model`](crates/svlang-model/), [`svlang-lint`](crates/svlang-lint/), [`svlang-lsp`](crates/svlang-lsp/) | owned SV/UVM/RAL model, 446-rule static linter & auto-fix engine, and LSP server |
| Browser | [`svlang-wasm`](crates/svlang-wasm/) | svlang as a WebAssembly module, with a playground page |
| Python | [`pysvlang`](crates/svlang-py/) | the same pipeline from Python |

## Quick start

Rust:

```rust
use svlang::{Compilation, LowerConfig, SyntaxKind};

let comp = Compilation::from_args(["-f", "sim/top.f", "-top", "top"])?;
for diag in comp.diagnostics() {
    eprintln!("{diag}");
}
for tree in comp.trees() {
    let modules = tree.preorder().into_iter().filter(|&n| tree.kind(n) == SyntaxKind::ModuleDecl).count();
    println!("{modules} modules");
}
println!("{}", comp.elaborate().hierarchy.join("\n"));
let netlist = comp.lower_netlist(&LowerConfig::default()).unwrap_netlist();
println!("{} registers", netlist.states.len());
```

Python:

```python
import pysvlang

comp = pysvlang.Compilation.from_args(["-f", "sim/top.f", "-top", "top"])
for diag in comp.diagnostics():
    print(diag)
for tree in comp.trees:
    for mod in tree.root.find_all("ModuleDecl"):
        print(mod.child("Name").bare_text, mod.location)
print("\n".join(comp.hierarchy()))
netlist = comp.lower_netlist()
print(len(netlist.states), "registers")
```

More: the [svlang crate README](crates/svlang/README.md) and its
[examples](crates/svlang/examples/) (a custom linter, hierarchy elaboration,
netlist lowering with an external SVA property), the
[pysvlang README](crates/svlang-py/README.md), and the API reference on
[docs.rs](https://docs.rs/svlang).

From the shell, with no code at all ([`svlang-cli`](crates/svlang-cli/)):

```sh
svlang check -f sim/top.f -top top                  # errors and warnings
svlang hier  -f sim/top.f -top top                  # the elaborated hierarchy
svlang btor2 -f sim/top.f -top top -o top.btor2     # for a model checker
```

## Status

Version 0.1: the API works and is tested, and it may still change before
1.0. [docs/architecture/rust.md](docs/architecture/rust.md) describes the
crates and the gates that compare the front end, construct by construct, with
[Slang](https://github.com/MikePopoloski/slang) as a reference. Slang is
never needed to build or use svlang.

On [sv-tests](https://github.com/chipsalliance/sv-tests), the public
SystemVerilog conformance suite, svlang passes all 1,024 tests it can judge;
[docs/reference/sv-tests.md](docs/reference/sv-tests.md) has the numbers by
clause.
[docs/reference/census.md](docs/reference/census.md) measures the other end:
how much open-source RTL lowers to a netlist, and what stops the rest.

## Tools built on svlang

All native tools ship inside the single unified **`svlang`** binary (`cargo install svlang-cli`):

- **`svlang lint`** ([`svlang-lint`](crates/svlang-lint/)): static analysis and auto-fix for SystemVerilog, UVM, RTL/FSM/CDC, and formal readiness (`--preset formal-ready`).
- **`svlang lsp`** ([`svlang-lsp`](crates/svlang-lsp/)): language server powering the [`svlang.svlang`](editors/vscode/) VS Code / Open VSX extension.
- **`svlang check`**, **`svlang filelist`**, **`svlang map`**, **`svlang hier`**, **`svlang netlist`**, **`svlang btor2`**, **`svlang aiger`**, **`svlang yosys`**: compile checking, filelist normalization, UVM testbench mapping, and RTL/SVA lowering.

# svlang lint

Static analysis for SystemVerilog (IEEE 1800) and UVM (IEEE 1800.2)
verification code: UVM methodology, OOP design, concurrency and delta-cycle
races, SVA and coverage health, constrained random, formal readiness (`SV-FPV-*`),
RTL/FSM/CDC/dataflow, code smells, duplication and style. Findings come as text,
JSON, SARIF 2.1.0 or a self-contained HTML report, with baselines, scoped
waivers, safe autofixes, a language server (`svlang lsp`) and a VS Code / Open
VSX extension (`svlang.svlang`).

Every rule has a `bad.sv` / `good.sv` pair under [`testdata/rules/`](testdata/rules/)
tested by `cargo test` and end-to-end suites under [`tests/`](tests/). The rule
catalog is in [docs/rules/README.md](docs/rules/README.md).

## Install

**A release build.** Tagged
[releases](https://github.com/mayurkubavat/svlang/releases) attach
`svlang-<platform>.tar.gz` for Linux x64/arm64 (glibc 2.28 or later),
macOS arm64/x64, and Windows x64: the single `svlang` binary and the Python SDK.
Unpack it and put its `bin/` on your `PATH`. The VS Code / Open VSX extension
(`svlang.svlang`) is published to both registries and attached per platform as
`svlang-<target>.vsix`.

**From source.** The unified `svlang` binary needs only a Rust toolchain (1.82+):

```sh
cargo install svlang-cli
# or from a checkout:
git clone https://github.com/mayurkubavat/svlang.git
cd svlang
cargo build --release -p svlang-cli   # -> target/release/svlang
```

## Quick start

```sh
# Lint a testbench with the methodology profile
svlang lint tb/*.sv -I tb --preset methodology

# JSON or SARIF reports for CI
svlang lint -f tb.f --json findings.json --sarif findings.sarif

# Pre-FPV formal-readiness checks and safe auto-fixes
svlang lint -f rtl.f --preset formal-ready

# Every rule: style, smells, advisory and experimental ones too
svlang lint -f tb.f --preset all --experimental
```

`svlang` takes simulator-style compile options (`-f`, `+incdir+`, `+define+`,
`-top`), reads `.svlang-lint/` project config, and honors waivers and baselines.
Optional dev-only Slang compiler dumpers live under `tools/oracles/`
(`cmake --preset oracles`) for differential IEEE 1800 conformance gates; see
[third_party/slang](third_party/slang/README.md).

## Rules

409 rules in 20 categories. The largest groups:

| Area | What it catches |
|---|---|
| UVM (94) | factory and `config_db` misuse, phasing and objections, sequences, RAL maps and adapters, deprecated API |
| Style, naming, maintainability (80) | layout, naming conventions, long routines, deep nesting, god classes, magic numbers, duplication |
| Correctness, concurrency (60) | width mismatches, delta-cycle races, fork/join and clocking pitfalls, unchecked failures |
| Randomization, coverage, SVA (56) | unchecked `randomize()`, constraint conflicts, coverage that cannot close, vacuous assertions |
| OOP, architecture (33) | missing `virtual`, unsafe casts, layer violations, coupling and reach-through |

By default the rules that find bugs and methodology problems run
(soundness, methodology, performance, security); style, maintainability
and advisory rules are opt-in with `--preset style`, `smells`, `advisory`
or `all`, or by configuring them in the project config. Profiles
(`--preset`) pick a set for a purpose, such as `methodology`. The
generated [rule catalog](docs/rules/README.md) lists every rule with its
severity, profiles and `bad.sv`/`good.sv` examples, and the
[UVM Methodology Guide](docs/methodology/README.md) explains the reasoning
behind the design rules.

The full svlang-lint guide (profiles, fixes, reports, Python rules, baselines,
waivers, CI) is [docs/guide/svlang-lint.md](docs/guide/svlang-lint.md). The Rust
tools are described in [docs/architecture/rust.md](docs/architecture/rust.md).

## Repository layout

```
crates/            Rust workspace members (Cargo.toml is at the root)
crates/svlang-py/  pysvlang: Python bindings for the svlang front end (PyO3 + maturin)
python/            Python SDK: rules, waivers and project config in Python (+ examples/, tests/)
editors/vscode/    VS Code / Open VSX extension (`svlang.svlang`)
tests/             end-to-end Python CLI, catalog, and --fix invariant suites
testdata/          rule and compiler fixtures: rules/, common/, conformance/, map/, model/, netlist/
tools/oracles/     optional dev-only Slang compiler dumpers (`cmake --preset oracles`)
tools/gates/       conformance, netlist, and differential compiler gates
tools/codegen/     rule catalog and guideline generators
tools/quality/     precision, error budget, config sweep, census
third_party/slang/ Slang pin for optional dev-only oracles
docs/              guides, architecture, rule catalog, methodology guide
.svlang-lint/      this repository's own lint config
```

[docs/README.md](docs/README.md) is the index of all documentation, and
[docs/architecture/overview.md](docs/architecture/overview.md) explains how
the pieces fit together.

## Use it in CI

The repository is a GitHub composite action ([action.yml](action.yml)) and a
pre-commit hook ([.pre-commit-hooks.yaml](.pre-commit-hooks.yaml)); see
[CI, pre-commit and editor](docs/guide/svlang-lint.md#ci-pre-commit-and-editor).

## Tests

```sh
cargo test                                         # all pure-Rust workspace tests
tools/gates/rust_gate.sh                           # guidelines, cargo fmt --check, Clippy -D warnings
python3 tools/gates/netlist_gate.py                # RTL netlist lowering gate
python3 tests/test_fix_invariants.py               # --fix mutation & idempotency invariants
python3 python/tests/test_python_rules.py          # Python rule SDK tests
```

## Citing

If you use this project, please cite the technical report:

> M. Kubavat, "InFormal: An Open-Source Formal Property Verifier for SystemVerilog with Checked Verdicts," technical report, Zenodo, Nov. 2026. doi: [10.5281/zenodo.24261270](https://doi.org/10.5281/zenodo.24261270)

```bibtex
@techreport{kubavat2026informal,
  author      = {Kubavat, Mayur},
  title       = {{InFormal}: An Open-Source Formal Property Verifier for {SystemVerilog} with Checked Verdicts},
  institution = {Zenodo},
  year        = {2026},
  month       = nov,
  doi         = {10.5281/zenodo.24261270},
  url         = {https://doi.org/10.5281/zenodo.24261270}
}
```

GitHub's "Cite this repository" button reads the same data from `CITATION.cff`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to add a rule and its tests.

## License

Apache License 2.0, see [LICENSE](LICENSE) and [NOTICE](NOTICE). Third-party components are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

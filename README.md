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
| All of it | [`svlang`](crates/svlang/) | one crate that re-exports the stages behind `Driver` and `Compilation` |
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

## Status

Version 0.1: the API works and is tested, and it may still change before
1.0. [docs/architecture/rust.md](docs/architecture/rust.md) describes the
crates and the gates that compare the front end, construct by construct, with
[Slang](https://github.com/MikePopoloski/slang) as a reference. Slang is
never needed to build or use svlang.

## Tools built on svlang

This repository also holds the DV tools that use the library:

- **UVMCheck** (`uvmcheck`, also built as `dv-lint`): static analysis for
  SystemVerilog and UVM testbenches, described below.
- **dv-lsp**: a language server, with a [VS Code extension](editors/vscode/).
- **dv-build**, **dv-map**, **dv-filelist**: compile checking, design
  mapping and filelist tools.

# UVMCheck

Static analysis for SystemVerilog (IEEE 1800) and UVM (IEEE 1800.2)
verification code: UVM methodology, OOP design, concurrency and delta-cycle
races, SVA and coverage health, constrained random, code smells, duplication
and style. Findings come as text, JSON, SARIF 2.1.0 or a self-contained HTML
report, with baselines, waivers, safe autofixes, a language server and a
VS Code extension.

There are two engines that share one rule set and one test suite:

| Engine | Where | Front end | Binary |
|---|---|---|---|
| **uvmcheck** (C++20) | [`cpp/`](cpp/) | [Slang](https://github.com/MikePopoloski/slang) | `build/uvmcheck` |
| **dv-lint** and the DV tools (Rust): what releases ship | [`crates/`](crates/) | [`svlang`](crates/svlang/) — pure-Rust lexer, preprocessor, parser and elaborator (Slang only as an opt-in reference) | `target/release/uvmcheck` (also built as `dv-lint`), `dv-lsp`, `dv-build`, `dv-map`, `dv-filelist` |

Every rule has a `bad.sv` / `good.sv` pair under [`testdata/rules/`](testdata/rules/)
that both engines are tested against, and parity gates check that their
findings match on real code. The rule catalog is in
[docs/rules/README.md](docs/rules/README.md).

## Install

**A release build.** Tagged
[releases](https://github.com/mayurkubavat/svlang/releases) attach
`uvmcheck-<platform>.tar.gz` for Linux x64 and arm64 (glibc 2.28 or later)
and macOS arm64 and x64: `uvmcheck` (the native engine), `dv-lsp`,
`dv-build`, `dv-map`, `dv-filelist` and the Python SDK. Unpack it and put
its `bin/` on your `PATH`. The VS Code extension is attached too.

**From source.** The native tools need only a Rust toolchain (1.82+):

```sh
git clone https://github.com/mayurkubavat/svlang.git
cd svlang
cargo build --release            # -> target/release/uvmcheck (also as dv-lint), dv-lsp, dv-build, dv-map, dv-filelist
```

The C++ reference engine on Slang is optional: the parity gates compare
against it, and it still serves the options the native engine does not take
yet. It needs a C++20
compiler (clang 15+, AppleClang 15+ or gcc 12+), CMake 3.28+, Ninja and
Python 3.9+, and fetches Slang at configure time; `ccache` is recommended.

```sh
cmake --preset reference
cmake --build build -j 4         # -> build/uvmcheck
```

In a checkout with both built, [`tools/front/uvmcheck`](tools/front/uvmcheck)
runs the native engine and falls back to the C++ binary for those options.

## Quick start

```sh
# Lint a testbench with the methodology profile
uvmcheck tb/*.sv -I tb --preset methodology

# The same with dv-lint directly, as JSON or SARIF for CI
target/release/dv-lint +incdir+tb tb/top.sv --preset methodology
target/release/dv-lint -f tb.f --json findings.json --sarif findings.sarif

# Every rule: style, smells, advisory and experimental ones too
target/release/dv-lint -f tb.f --preset all --experimental
```

Both engines take simulator-style compile options (`-f`, `+incdir+`,
`+define+`, `-top`), read `.uvmcheck/` project config, and honor waivers and
baselines. The Rust tools reach Slang only for the parity gates, behind the
`slang` Cargo feature (off by default); see
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

The full uvmcheck guide (profiles, fixes, reports, Python rules, baselines,
waivers, CI) is [docs/guide/uvmcheck.md](docs/guide/uvmcheck.md). The Rust
tools are described in [docs/architecture/rust.md](docs/architecture/rust.md).

## Repository layout

```
cpp/               uvmcheck: src/, include/uvmcheck/, tests/ (drivers of the binary)
crates/            Rust workspace members (Cargo.toml is at the root)
crates/svlang-py/  pysvlang: Python bindings for the svlang front end (PyO3 + maturin)
python/            Python SDK: rules, waivers and project config in Python (+ examples/, tests/)
editors/vscode/    VS Code / Cursor extension
testdata/          fixtures both engines read: rules/, common/, conformance/, map/, model/
data/uvm-api/      UVM API packs, compiled into both engines
tools/oracles/     Slang reference dumpers the gates compare against (built by CMake)
tools/gates/       parity and conformance gates
tools/codegen/     generators run by the build and for the docs
tools/quality/     precision, config sweep, AST coverage, Slang canary
third_party/slang/ the Slang pin, local patches (none today) and how to update it
docs/              guides, architecture, rule catalog, methodology guide
.uvmcheck/         this repository's own lint config
```

[docs/README.md](docs/README.md) is the index of all documentation, and
[docs/architecture/overview.md](docs/architecture/overview.md) explains how
the pieces fit together.

## Use it in CI

The repository is a GitHub composite action ([action.yml](action.yml)) and a
pre-commit hook ([.pre-commit-hooks.yaml](.pre-commit-hooks.yaml)); see
[CI, pre-commit and editor](docs/guide/uvmcheck.md#ci-pre-commit-and-editor).

## Tests

```sh
ctest --test-dir build                 # uvmcheck suites
ctest --test-dir build-model           # + C ABI, Python binding, cargo test --features slang
python3 cpp/tests/regression.py --rule UVM-CFG-002
cargo test                             # Rust, native only
tools/gates/rust_gate.sh               # cargo fmt --check + Clippy -D warnings
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to add a rule and its tests.

## License

Apache License 2.0, see [LICENSE](LICENSE) and [NOTICE](NOTICE). Third-party components are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

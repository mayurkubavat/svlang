# svlang

[![crates.io](https://img.shields.io/crates/v/svlang.svg)](https://crates.io/crates/svlang)
[![docs.rs](https://img.shields.io/docsrs/svlang)](https://docs.rs/svlang)
[![PyPI](https://img.shields.io/pypi/v/pysvlang.svg)](https://pypi.org/project/pysvlang/)
[![VS Code Marketplace](https://img.shields.io/visual-studio-marketplace/v/svlang.svlang?label=VS%20Code%20Marketplace)](https://marketplace.visualstudio.com/items?itemName=svlang.svlang)
[![Open VSX](https://img.shields.io/open-vsx/v/svlang/svlang?label=Open%20VSX)](https://open-vsx.org/extension/svlang/svlang)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Pure-Rust **IEEE 1800-2023 SystemVerilog** and **IEEE 1800.2 UVM** compiler SDK, word-level RTL/SVA netlist lowerer, 475-rule static linter, and Language Server (`svlang`). Self-contained with zero C++ build or runtime dependencies.

- **IDE Extension ([`svlang.svlang`](editors/vscode/README.md))**: VS Code, Cursor & Antigravity — compiler-backed semantic highlighting, `` `ifdef `` dimming, RTL hierarchy & signal driver/load tracing, live syntax/semantic checks (`CMP-ERR-001`), and on-demand or on-save static linting with one-click quick fixes.
- **Unified CLI ([`svlang-cli`](crates/svlang-cli/README.md))**: one native `svlang` binary for `check`, `lint`, `lsp`, `filelist`, `map`, `hier`, `cst`, `netlist`, `btor2`, `aiger`, and `yosys`.
- **Compiler & Netlist SDK ([`svlang`](crates/svlang/README.md) / [`pysvlang`](crates/svlang-py/README.md))**: simulator filelists, preprocessing, lossless CSTs, elaboration, IEEE 1800 §11.6/§11.8 expression typing, SVA automata, and word-level RTL netlists from Rust or Python.

---

## Install

```sh
# Unified CLI (`svlang check`, `svlang lint`, `svlang lsp`, `svlang btor2`, ...)
cargo install svlang-cli

# Rust & Python compiler SDKs
cargo add svlang
pip install pysvlang
```

Prebuilt `svlang-<target>.tar.gz` archives and self-contained `svlang-<target>.vsix` bundles (`linux-x64`, `linux-arm64`, `darwin-arm64`, `darwin-x64`, `win32-x64`) are attached to every [GitHub Release](https://github.com/mayurkubavat/svlang/releases), and the editor extension is published on [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=svlang.svlang) and [Open VSX](https://open-vsx.org/extension/svlang/svlang).

---

## Quick Start

### 1. Command Line (`svlang`)

```sh
# Compiler syntax, elaboration, and IEEE 1800 semantic checks
svlang check -f sim/top.f -top top

# Static lint (default: soundness, methodology, performance, security)
svlang lint -f sim/tb.f

# Target specific 3-3-3 domains/groups, formal readiness, or auto-fix
svlang lint -f sim/rtl.f --rule 'RTL-*,SVA-*'
svlang lint -f sim/rtl.f --preset formal-ready
svlang lint -f sim/tb.f  --fix --sarif results.sarif --html report.html

# Inspect any rule's documentation, options, and bad/good examples
svlang lint --explain RTL-FSM-002

# Elaborate hierarchy or lower RTL + SVA to Btor2 / AIGER / Yosys JSON
svlang hier  -f sim/top.f -top top
svlang btor2 -f sim/top.f -top top -o top.btor2
```

### 2. Rust SDK (`svlang`)

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

### 3. Python SDK (`pysvlang`)

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

See [`crates/svlang/README.md`](crates/svlang/README.md), [`crates/svlang/examples/`](crates/svlang/examples/), [`crates/svlang-py/README.md`](crates/svlang-py/README.md), and [docs.rs/svlang](https://docs.rs/svlang) for full API details.

---

## Lint Rule Taxonomy (`DOM-GRP-NNN`)

`svlang lint` organizes **475 rules and sub-rules** across **5 top-level EDA domains**, **34 3-letter groups**, and **18 categories** using a strict **`DOM-GRP-NNN`** (`3-3-3`) signature scheme:

| Domain | Selector | Groups (`DOM-GRP`) | Rules | Focus |
| :--- | :--- | :--- | ---: | :--- |
| **Compiler** | `CMP-*` | `CMP-ERR` | 2 | Preprocessor, parser, elaboration, and IEEE 1800 type diagnostics |
| **RTL Design** | `RTL-*` | `RTL-SYN`, `RTL-FSM`, `RTL-CDC`, `RTL-DAT` | 17 | Synthesizable RTL, resets, latches, FSM deadlocks/reachability, CDC, X-optimism & width hazards |
| **SystemVerilog** | `SVL-*` | `SVL-COR`, `SVL-TYP`, `SVL-OOP`, `SVL-CON`, `SVL-RND`, `SVL-COV`, `SVL-PRF`, `SVL-DED`, `SVL-MNT`, `SVL-WVR`, `SVL-PRT`, `SVL-DPI`, `SVL-SEC`, `SVL-DEP`, `SVL-STY`, `SVL-NAM` | 296 | Language correctness, OOP, concurrency/races, solver constraints, coverage, performance, dead code, smells, DPI-C, security, style & naming |
| **Assertions & Formal** | `SVA-*` | `SVA-PRP`, `SVA-FPV` | 21 | Concurrent/immediate SVA clocks, resets, action blocks, vacuity, and pre-FPV formal readiness (`--preset formal-ready`) |
| **IEEE 1800.2 UVM** | `UVM-*` | `UVM-FAC`, `UVM-PHS`, `UVM-OBJ`, `UVM-CFG`, `UVM-SEQ`, `UVM-TLM`, `UVM-CMP`, `UVM-RAL`, `UVM-DEP`, `UVM-PRT`, `UVM-NAM` | 139 | Factory, phasing, inter-procedural objection CFG, `uvm_config_db`, sequences, TLM/scoreboards, component roles, RAL register maps & UVM naming |

- **Noise-free defaults**: Running `svlang lint` with no flags runs bug-finding and methodology profiles (`soundness`, `methodology`, `performance`, `security`). Opinionated formatting (`style`), maintainability metrics (`smells`), and `advisory` rules stay opt-in via `--preset style|smells|advisory|all` or `.svlang-lint/config.py`.
- **Prefix wildcards everywhere**: Select, fix, ignore, or waive whole domains or groups (`--rule 'RTL-*'`, `--fix-rule 'SVL-STY-*'`, `svlang.lint.ignore: ["SVL-NAM-*", "UVM-NAM-*"]`, or `// svlang-lint:off SVL-STY-*`).
- **Full references**: [Generated Rule Catalog](docs/rules/README.md) · [UVM Methodology Guide (`MG-1`..`MG-9`)](docs/methodology/README.md) · [Linter User Guide](docs/guide/svlang-lint.md) · [Project Configuration](docs/guide/project_config.md).

---

## Workspace Architecture

| Stage | Crate | Responsibility |
| :--- | :--- | :--- |
| Filelists | [`sv-source`](crates/sv-source/) | VCS/Xcelium `-f`/`-F` lists, `$VAR` expansion, `+incdir+`, `+define+`, `-G`, `-y`/`-v` |
| Lexer | [`sv-lexer`](crates/sv-lexer/) | Lossless tokenizer preserving every source byte and trivia span |
| Preprocessor | [`sv-preprocessor`](crates/sv-preprocessor/) | Clause 22 macros, `` `include ``, `` `ifdef ``, and token expansion provenance |
| Syntax | [`sv-syntax`](crates/sv-syntax/) | Error-recovering lossless concrete syntax tree (CST) |
| Primitives | [`sv-bigint`](crates/sv-bigint/), [`sv-fxhash`](crates/sv-fxhash/), [`sv-scopes`](crates/sv-scopes/), [`sv-ir`](crates/sv-ir/) | Arbitrary-width 2's-complement integers, fast hashing, symbol scopes, and shared elaborated IR |
| Semantic & SVA | [`sv-checks`](crates/sv-checks/), [`sv-expr`](crates/sv-expr/), [`sv-sva`](crates/sv-sva/), [`sv-semantic`](crates/sv-semantic/) | Clause-by-clause semantic checks, 4-state constant eval, elaboration, IEEE 1800 §11.6/§11.8 expression typing, and bound SVA trees |
| RTL Lowering | [`sv-netlist`](crates/sv-netlist/), [`sv-btor2`](crates/sv-btor2/), [`sv-aiger`](crates/sv-aiger/), [`sv-yosys`](crates/sv-yosys/) | Word-level RTL & SVA NFA/DFA lowering, memory blasting, Btor2, AIGER, and Yosys JSON backends |
| Umbrella & Bindings | [`svlang`](crates/svlang/), [`pysvlang`](crates/svlang-py/), [`svlang-wasm`](crates/svlang-wasm/) | Unified Rust `Driver`/`Compilation` API, Python wheels, and WebAssembly playground |
| Model, Lint, LSP & CLI | [`svlang-model`](crates/svlang-model/), [`svlang-lint`](crates/svlang-lint/), [`svlang-lsp`](crates/svlang-lsp/), [`svlang-cli`](crates/svlang-cli/) | Owned SV/UVM/RAL model, parallel 475-rule linter & autofix engine, LSP server, and unified `svlang` CLI |

### Conformance & Verification Gates

- **[sv-tests](docs/reference/sv-tests.md)**: Passes all 1,024 applicable tests in the public SystemVerilog conformance suite.
- **[RTL Netlist Census](docs/reference/census.md)** & `netlist_gate.py`: End-to-end word-level lowering and SVA automaton verification.
- **Differential Oracles ([`third_party/slang`](third_party/slang/README.md))**: Optional dev-only Slang dumpers (`cmake --preset oracles`) cross-check preprocessing, CSTs, symbol binding, specialization counts, and expression widths/signedness; Slang is never needed to build or run `svlang`.

```sh
cargo test                                         # all pure-Rust workspace tests
./tools/gates/rust_gate.sh                         # guidelines, cargo fmt --check, Clippy -D warnings
python3 tools/gates/netlist_gate.py                # RTL & SVA netlist lowering gate
python3 tests/test_fix_invariants.py               # --fix mutation & idempotency invariants
python3 python/tests/test_python_rules.py          # Python rule SDK tests
```

---

## Documentation & CI Integration

- **Documentation Index**: [docs/README.md](docs/README.md) ([Architecture](docs/architecture/rust.md), [Linter Guide](docs/guide/svlang-lint.md), [Project Config](docs/guide/project_config.md), [Python Rule SDK](docs/guide/python_rule_sdk.md), [VS Code Extension](editors/vscode/README.md)).
- **GitHub Action & pre-commit**: Use [`action.yml`](action.yml) for PR changed-line gating + SARIF upload and [`.pre-commit-hooks.yaml`](.pre-commit-hooks.yaml) for local commit hooks; see [CI, pre-commit and editor](docs/guide/svlang-lint.md#ci-pre-commit-and-editor).

---

## Citing

If you use this project in research or industrial publications, please cite:

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

## Contributing & License

See [CONTRIBUTING.md](CONTRIBUTING.md) for rule authoring, prefix allocation, and verification gates. Licensed under Apache License 2.0 ([LICENSE](LICENSE), [NOTICE](NOTICE), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)).

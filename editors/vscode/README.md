# svlang for VS Code, Cursor & Antigravity

IEEE 1800-2023 SystemVerilog and UVM language server, semantic code browser, and real-time static linter powered by **svlang**'s pure-Rust compiler front end (`dv-lsp`).

## Features

- **Real-Time Diagnostics (445+ Rules)**: Instant feedback across UVM methodology, OOP soundness, randomization & constraints, RTL/FSM/CDC/dataflow, SVA, functional coverage, dead code, and style.
- **Formal-Readiness Pack (`formal-ready`)**: Built-in pre-FPV checks (`SV-FPV-001..005`) with quick fixes (including `assume` -> `assert` on DUT outputs and auto-injecting `disable iff (<reset>)`).
- **Deep Semantic Analysis**: Cross-component validation of objection CFGs, Config DB type matching, virtual sequencer wiring, and RAL predictor binding.
- **Project-Aware Linting**: Compiles from `.svlang-lint/config.py` or `.svlang-lint.json` (sources, filelists, include dirs, defines) over your unsaved edits, honoring scoped waivers (`svlang-lint:waive`, `svlang-lint:off`/`on`, `verissimo_lint:*`) and baselines.
- **Quick Fixes**: Apply safe rule fix-its, formal-readiness quick fixes, or waive findings directly from the editor.
- **Semantic Code Browsing**: Document outline, workspace symbols (`Ctrl/Cmd+T`), go to definition / declaration / type definition (including `extern` prototypes, macros, and base classes), find references, document highlights, and hover docs.
- **Context-Aware Completion**: In-scope names with inherited class members, `obj.` and `Class::`/`pkg::` members, `type_id::create`, `` ` `` macros (workspace and UVM), and `$` system tasks.
- **Syntax Highlighting & UVM Snippets**: Built-in SystemVerilog/Verilog grammars plus snippets (`uvm_component`, `uvm_object`, `uvm_sequence_item`, `uvm_sequence`, `uvm_driver`, `uvm_monitor`, `uvm_test`, `uvm_info`, `uvm_error`, `cfgget`, `always_ff`, `module`).

## Supported Platforms

Platform `.vsix` releases bundle a self-contained native `dv-lsp` binary—no external compiler, Python, or Rust toolchain required:

- Linux (`linux-x64`, `linux-arm64` — glibc 2.28+)
- macOS (`darwin-arm64`, `darwin-x64`)
- Windows (`win32-x64`)

To use your own `dv-lsp` build (or run on another platform), put `dv-lsp` on `PATH` or set `svlang.serverPath`:

```bash
git clone https://github.com/mayurkubavat/svlang.git
cd svlang
cargo build --release -p dv-lsp
export PATH="$(pwd)/target/release:$PATH"
```

To lint with the C++ Slang reference engine instead, set `svlang.executablePath` to a `svlang-lint-slang` binary (`dv-lsp` then continues serving code browsing).

## Configuration Settings

Configure via User or Workspace `settings.json`:

| Setting | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `svlang.enable` | `boolean` | `true` | Enable/disable live linting diagnostics and quick fixes |
| `svlang.preset` | `string` | `""` | Default lint preset when not set in project config (e.g. `default`, `formal-ready`, `all`, `soundness`, `style`) |
| `svlang.serverPath` | `string` | `"dv-lsp"` | Path to the `dv-lsp` binary (defaults to the bundled binary, else `PATH`) |
| `svlang.executablePath` | `string` | `""` | Optional path to `svlang-lint-slang` to lint with the C++ Slang engine |
| `svlang.navigation.enable` | `boolean` | `true` | Outline, definitions, references, hover, and completion |
| `svlang.navigation.libraryPaths` | `string[]` | `[]` | Extra trees to index (UVM, VIPs; `$VARS` and `~` expanded) |
| `svlang.navigation.useUvmHome` | `boolean` | `true` | Also index `$UVM_HOME/src` when `UVM_HOME` is set |
| `svlang.navigation.exclude` | `string[]` | `[]` | Directory names or path endings to skip when indexing |
| `svlang.trace.server` | `string` | `"off"` | LSP protocol trace logging (`off`, `messages`, `verbose`) |

*(Legacy `svlang-lint.*` settings are still read as a fallback if present.)*

## Commands

- **`svlang: Restart Language Servers`**: Restarts the language server, reloading project config, waivers, and workspace index.

## Installing

Install **`svlang.svlang`** from the **VS Code Marketplace** or **Open VSX Registry** (Cursor, Antigravity, VSCodium, Windsurf), or via CLI:

```bash
code --install-extension svlang.svlang      # VS Code
cursor --install-extension svlang.svlang    # Cursor
```

Or build a self-contained `.vsix` locally:

```bash
cargo build --release -p dv-lsp
cd editors/vscode
npm ci
npm run bundle:local
npm run package
```

## Development

```bash
cd editors/vscode
npm ci
npm run compile
# Press F5 in VS Code to launch the Extension Development Host
```

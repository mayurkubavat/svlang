# svlang-lint Extension for VS Code, Cursor & Antigravity

<img src="images/svlang-lint.png" width="96" alt="svlang-lint logo">

Real-time SystemVerilog and UVM static analysis in your editor, powered by **svlang-lint**'s native engine.

## Features

- **Real-Time Diagnostics**: Instant feedback on 330+ rules covering UVM methodology, OOP soundness, race conditions, TLM connectivity, and dead code.
- **Deep Semantic Analysis**: Cross-component validation of objection CFGs, Config DB type matching, virtual sequencer wiring, and RAL predictor binding.
- **Live linting**: Diagnostics as you type from the language server (`dv-lsp`), checked in the background once typing pauses, so browsing stays responsive.
- **Project aware**: Compiles the project from `.svlang-lint/config.py` (sources, file lists, include dirs, defines) with your unsaved edits, and applies its waivers and baseline.
- **Quick fixes**: Apply a rule's fix-it, or waive a finding with a comment on the line above.
- **Code browsing** (the same server): outline, workspace symbols (Ctrl/Cmd+T), go to definition / declaration / type definition (including `extern` prototype and out-of-block body pairs, macros and base classes), find references, highlights, and hover with the declaration and its doc comment.
- **Completion**: names in scope with inherited class members, `obj.` and `Class::`/`pkg::` members, `type_id::create`, `` ` `` macros (workspace and UVM) and `$` system tasks.
- **Syntax highlighting**: Built-in SystemVerilog/Verilog grammar with UVM classes, macros, phases and verbosity constants; no other SV extension needed.
- **UVM snippets**: `uvm_component`, `uvm_object`, `uvm_sequence_item`, `uvm_sequence`, `uvm_driver`, `uvm_monitor`, `uvm_test`, `uvm_info`, `uvm_error`, `cfgget`, `always_ff`, `module`.

## Requirements

The extension runs one server, `dv-lsp`: lint diagnostics and quick fixes, code browsing and completion. Release builds of the extension (`.vsix` per platform, attached to each GitHub release) bundle it for Linux x64 and macOS arm64; nothing else is needed. On other platforms, or to use your own build, put `dv-lsp` on `PATH` or set `svlang-lint.navigation.serverPath`:

```bash
git clone https://github.com/mayurkubavat/svlang.git
cd svlang
cargo build --release -p dv-lsp
export PATH="$(pwd)/target/release:$PATH"
```

`npm run bundle:local` before `npm run package` ships your build inside the `.vsix`. To lint with the Slang reference engine instead, set `svlang-lint.executablePath` to a `svlang-lint-slang` binary; `dv-lsp` then serves code browsing only.

## Configuration Settings

You can customize svlang-lint behavior via `.vscode/settings.json` or Cursor Settings:

| Setting | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `svlang-lint.enable` | `boolean` | `true` | Enable/disable live linting |
| `svlang-lint.executablePath` | `string` | `""` | Only to lint with the Slang engine: path to a `svlang-lint-slang` binary |
| `svlang-lint.navigation.enable` | `boolean` | `true` | Outline, definitions, references, hover and completion |
| `svlang-lint.navigation.serverPath` | `string` | `"dv-lsp"` | Path to the `dv-lsp` binary (lint and code browsing) |
| `svlang-lint.navigation.libraryPaths` | `string[]` | `[]` | Extra trees to index (UVM, VIPs); `$UVM_HOME/src` is added when set |
| `svlang-lint.navigation.exclude` | `string[]` | `[]` | Directory names to skip when indexing |
| `svlang-lint.trace.server` | `string` | `"off"` | LSP protocol trace logging (`off`, `messages`, `verbose`) |

## Commands

- **`svlang-lint: Restart Language Servers`**: Restarts both servers, reloading the project config and the index.

## Installing

The extension is one `.vsix` file that works in VS Code and in its forks (Cursor, Antigravity, VSCodium, Windsurf).

```bash
cd editors/vscode
npm install
npm run package          # writes svlang-lint-vscode-<version>.vsix
code --install-extension svlang-lint-vscode-*.vsix      # VS Code
cursor --install-extension svlang-lint-vscode-*.vsix    # Cursor
```

In any of these editors you can also use **Extensions > ... > Install from VSIX...**. Cursor and Antigravity read the Open VSX registry rather than the Microsoft marketplace; `npm run publish:openvsx` (with `OVSX_PAT` set) publishes there when you want it listed.

## Installation for Development

```bash
cd editors/vscode
npm install
npm run compile
# Press F5 in VS Code to launch the Extension Development Host
```

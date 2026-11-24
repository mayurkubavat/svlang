# svlang for VS Code, Cursor & Antigravity

IEEE 1800-2023 SystemVerilog and UVM language server, compiler-backed semantic highlighter, RTL/signal tracer, and static linter (`474` rules + quick fixes) powered by **svlang**'s self-contained pure-Rust engine (`svlang lsp`).

---

## Quick Start (60 Seconds)

1. **Open any `.sv`, `.svh`, `.v`, `.vh`, or `.f` file** — compiler-backed semantic highlighting, inactive `` `ifdef `` dimming, hover docs, completion, go-to-definition (`F12` / `Cmd+Click`), and live **IEEE 1800-2023 syntax & semantic diagnostics** start automatically with **zero external setup**, while the 474 opinionated/methodology lint rules stay quiet until you want them.
2. **Use the `svlang` Status Bar item** (bottom-right) — click `[SV] svlang` anytime to open the **Control Center**:
   - **Lint Current File / Project Now** (run full static lint rules on demand)
   - **Set Lint Trigger** (`Off` default / `On Save` / `On Type`)
   - **Switch Lint Preset** (`default`, `formal-ready`, `soundness`, `methodology`, `style`, `all`, …)
   - **Set Preprocessor Defines** (live-updates `` `ifdef `` dimming and elaboration)
   - **Initialize Project Config** (`svlang lint --init` — auto-discovers `.f` filelists, `$UVM_HOME`, and naming conventions, writing `.svlang-lint/config.py`)
3. **Trace RTL & UVM code**:
   - **Signal Drivers & Loads**: Right-click any signal, port, variable, or class field → **`svlang: Trace Signal Drivers & Loads`** (opens **Explorer → Signal Trace** separating **Drivers / Writes** from **Loads / Reads**).
   - **RTL Module Hierarchy**: Open **Explorer → RTL Hierarchy** to browse your top-to-leaf module/interface instantiation tree.
   - **UVM Type & Call Hierarchy**: Right-click any class or task/function → **Show Type Hierarchy** / **Show Call Hierarchy**.

---

## Key Features

- **Compiler-Backed Highlighting + `` `ifdef `` Dimming**: Full LSP semantic tokens (`class`, `interface`, `namespace`, `type`, `enumMember`, `parameter`, `property`, `method`, `macro`) plus configurable dimming of untaken `` `ifdef `` / `` `ifndef `` / `` `else `` regions.
- **RTL Hierarchy & Signal Driver/Load Tracing**: Built-in Explorer panes separating procedural/continuous/output-port **Drivers** from **Loads**, plus module instantiation trees.
- **474 Verification & RTL Rules**: UVM factory/phasing/objections/config_db/TLM, formal readiness (`formal-ready`: `SV-FPV-001..005`), randomization & constraint solver checks, RTL/FSM/CDC/dataflow, SVA, coverage, and style.
- **One-Click Quick Fixes (`Cmd+.` / `Ctrl+.`)**:
  - Apply safe rule rewrites (or auto-apply all safe fixes on save with `"editor.codeActionsOnSave": { "source.fixAll.svlang": "explicit" }`).
  - Insert scoped waivers (`// svlang-lint-disable-next-line <RULE>`).
  - **Ignore `<RULE>` in Workspace or User Settings** with a single click.
- **Simulator `.f` Filelist Support**: Built-in syntax highlighting for `.f` / `.F` / `.vc` command files and lightweight `svlang.compile.filelists` configuration.

---

## Keyboard Shortcuts & Editor Actions

| Action | macOS | Linux / Windows |
| :--- | :--- | :--- |
| Quick Fix / Waive / Ignore Rule | `Cmd + .` | `Ctrl + .` |
| Go to Definition / Declaration | `F12` or `Cmd + Click` | `F12` or `Ctrl + Click` |
| Find All References | `Shift + F12` | `Shift + F12` |
| Trace Signal Drivers & Loads | Right-click → `Trace Signal Drivers & Loads` | Right-click → `Trace Signal Drivers & Loads` |
| Document Outline | `Cmd + Shift + O` | `Ctrl + Shift + O` |
| Workspace Symbols | `Cmd + T` | `Ctrl + T` |
| Control Menu | Click `svlang` in Status Bar | Click `svlang` in Status Bar |

---

## Configuration Hierarchy (Global vs. Workspace vs. Repo)

`svlang` follows a clean 3-layer configuration model (inspired by Ruff, clangd, and Biome):

1. **Repo `.svlang-lint/config.py` (Checked into Git — Team & CI Truth)**: Wins by default (`svlang.lint.configPreference = "filesystemFirst"`) so editor diagnostics match CI (`svlang lint`) 1:1.
2. **Workspace `.vscode/settings.json`**: Point at existing simulator `.f` filelists (`svlang.compile.filelists`), set active `` `ifdef `` defines (`svlang.compile.defines`), or switch `svlang.lint.trigger` to `"onSave"`.
3. **Global User `settings.json`**: Personal defaults across every repo — for example, set `"svlang.lint.requireConfig": true` to stay in quiet navigation-only mode in unconfigured folders, or set `"svlang.lint.ignore": ["SV-STY-104", "SV-STY-009"]` globally.

### Settings Reference (`Settings → Extensions → svlang`)

#### 1. Linting & Diagnostics
| Setting | Default | Description |
| :--- | :--- | :--- |
| `svlang.enable` | `true` | Master switch for diagnostics (navigation & highlighting stay on when `false`) |
| `svlang.diagnostics.syntaxAndSemantics` | `true` | Live IEEE 1800-2023 syntax & semantic compiler checks even when `lint.trigger` is `"off"` |
| `svlang.lint.trigger` | `"off"` | `"off"` (Mode 1: browsing + syntax/semantics; manual `Lint Now`), `"onSave"`, or `"onType"` |
| `svlang.lint.requireConfig` | `false` | Only auto-lint workspaces with `.svlang-lint/config.py` (or `lint.configPath`) |
| `svlang.lint.configPath` | `""` | Optional shared/global `config.py` path (`~` and `$VARS` expanded) |
| `svlang.lint.configPreference` | `"filesystemFirst"` | `"filesystemFirst"` (repo config wins) or `"editorFirst"` (VS Code settings win) |
| `svlang.preset` | `""` | Rule preset (`default`, `formal-ready`, `soundness`, `methodology`, `style`, `all`, …) |
| `svlang.lint.select` | `[]` | Additional rule IDs/names to enable |
| `svlang.lint.ignore` | `[]` | Rule IDs/names to silence in User or Workspace settings |

#### 2. Preprocessor, Filelists & Files
| Setting | Default | Description |
| :--- | :--- | :--- |
| `svlang.files.extraExtensions` | `[]` | Extra extensions treated as SystemVerilog (e.g. `[".sva", ".inc", ".vams"]`) |
| `svlang.compile.filelists` | `[]` | Simulator `.f` command files to compile when no `config.py` exists |
| `svlang.compile.defines` | `[]` | Active preprocessor defines (`["SYNTHESIS", "WIDTH=64"]`) for lint & `` `ifdef `` dimming |
| `svlang.compile.includeDirs` | `[]` | Extra `+incdir+` directories (`~` and `$VARS` expanded) |
| `svlang.inactiveRegions.enable` | `true` | Dim untaken `` `ifdef `` / `` `ifndef `` / `` `else `` regions |
| `svlang.inactiveRegions.opacity` | `0.5` | Opacity (`0.15`–`1.0`) for dimmed inactive preprocessor regions |

#### 3. Navigation, Highlighting & Server
| Setting | Default | Description |
| :--- | :--- | :--- |
| `svlang.navigation.enable` | `true` | Outline, workspace symbols, definitions, references, hierarchies, hover, completion |
| `svlang.semanticHighlighting.enable` | `true` | Compiler-backed LSP semantic token highlighting |
| `svlang.navigation.libraryPaths` | `[]` | Extra trees to index (UVM, VIPs; `~` and `$VARS` expanded) |
| `svlang.navigation.useUvmHome` | `true` | Automatically index `$UVM_HOME/src` when `UVM_HOME` is set |
| `svlang.navigation.exclude` | `[]` | Directory names or path endings to skip when indexing |
| `svlang.statusBar.show` | `"whenSvOpen"` | `"whenSvOpen"`, `"always"`, or `"never"` |
| `svlang.serverPath` | `"svlang"` | Path to `svlang` binary (defaults to bundled platform binary, else `PATH`) |
| `svlang.trace.server` | `"off"` | LSP protocol trace logging (`"off"`, `"messages"`, `"verbose"`) |

---

## Supported Platforms

Platform `.vsix` packages bundle the self-contained native `svlang` binary — no external C++, Python, or Rust toolchain required:
- Linux (`linux-x64`, `linux-arm64` — glibc 2.28+)
- macOS (`darwin-arm64`, `darwin-x64`)
- Windows (`win32-x64`)

## License

Apache-2.0 — see `LICENSE.txt` and `NOTICE`.

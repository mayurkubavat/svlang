# Changelog

## 0.6.2

- Bundles svlang 0.6.2 (netlist and Btor2 fixes; no extension changes).

## 0.6.1

- **Eliminated Multi-Repo Extension-Host Freeze Loop (`svlang/rtlHierarchy` & Async Indexing)**:
  - Refactored **Explorer → RTL Hierarchy** to lazy 1-level-at-a-time expansion (`Collapsed` by default, fetched only when the RTL Hierarchy view is visible and scoped to the active editor's subproject), cutting multi-repo payload size from `72+ MB` to `< 1 KB`.
  - Made large-workspace LSP indexing non-blocking with automatic subproject on-demand warming, expanded build/vendor/corpus directory exclusions (`references`, `corpus`, `third_party`, `vendor`, `build*`, `obj_dir`, `csrc`, `simv.daidir`, `xcelium.d`, `work`, `informal_work`), and sped up `textDocument/semanticTokens/full` by `> 100x` via precomputed scope symbol tables.
- **Fixed `Go to Definition` on `class A extends B` & Package Headers**:
  - Added git/project-root and directory proximity ranking so multi-repo symbol collisions always resolve to the active project or `$UVM_HOME/src` first (with `~/.cache/svlang/uvm-core-*/src` fallback when GUI editors lack shell `$UVM_HOME`).
  - Indexed `` `include "file.svh" `` headers inside `package` scopes (`extends pkg::B` and `import pkg::*;`), `type` parameters (`class A #(type B = base_c) extends B`), and `extends`/`implements` type-clause filtering so class variables or same-named wrapper classes (`class B extends pkg::B`) never shadow base types.

## 0.6.0

- **Strict 2-Tier `DOM-GRP-NNN` (`3-3-3`) Rule Taxonomy**: Standardized all 475 diagnostics across 5 domains (`CMP-*`, `RTL-*`, `SVL-*`, `SVA-*`, `UVM-*`) and 34 prefixes (`CMP-ERR-001` for live compiler errors, `RTL-FSM-*`, `SVL-CON-*`, `SVA-FPV-*`, `UVM-RAL-*`, etc.).
- **Prefix Wildcard Rule Selection & Ignoring**: `svlang.lint.select`, `svlang.lint.ignore`, and inline/block waiver directives (`// svlang-lint:waive`, `// svlang-lint:off`) now accept domain and group wildcards such as `"RTL-*"`, `"SVL-NAM-*"`, `"UVM-NAM-*"`, and `"SVA-*"`.
- **Redesigned Extension Documentation**: Updated Marketplace / Open VSX overview, operating modes, rule domain matrix, and configuration examples.

## 0.5.2

- **Mode 1 Default (Code Browsing + Live Syntax/Semantics, On-Demand Lint)**: Changed `svlang.lint.trigger` default to `"off"` and added `svlang.diagnostics.syntaxAndSemantics` (default `true`). Code navigation, semantic highlighting, `` `ifdef `` dimming, RTL/signal tracing, and IEEE 1800-2023 syntax/semantic checks work out of the box while the 474 static lint rules stay quiet until `Lint Now` or `onSave`/`onType` is enabled.
- **IEEE 1800, RTL & UVM Rule Precision Overhaul**: Fixed 19 lint rules (including `SV-BAN-001` leaving `wire` nets intact, `SV-TIM-002` unitless `#delay` detection, `SV-FSM-002/003` clocked FSM identification, `SV-ASC-001` associative array checks, `SV-DEP-003`, `SV-TYP-001/008`, `SV-PRF-010`, `SV-COR-005/020`, `SVA-RST-001`, and standalone `.svh` handling) and moved house-style/advisory rules out of default presets.
- **Bundled `svlang` 0.5.2 Elaboration Fixes**: Includes context-determined `**` base sizing in parameters (`localparam logic [51:0] P = 2**51`) and instance-scoped type-parameter sizing (`child #(.T(out_t))`).

## 0.5.1

- **Bundled `svlang` 0.5.1 language server**: picks up three elaboration fixes (generate conditions with a reduction operator, `'1` as a sized parameter default, selects on packed struct members with their own range). No extension settings or commands changed.

## 0.5.0

- **Bundled `svlang` 0.5.0 language server**: picks up the compiler front-end fixes of the 0.5.0 release (unnamed UDP instances, transition-bin repetitions, constraints that start with a concatenation, based literals split across lines, `default clocking <name>;`, implicit `genblk<n>` names, `` `begin_keywords `` names) and three new diagnostics (unnamed module instance, import of a name the package does not declare, assignment to a `const` variable). No extension settings or commands changed.

## 0.4.1

- **Custom Monochrome `$(svlang-logo)` Status Bar Icon**: Added a dedicated die-corner `SV` WOFF icon font (`contributes.icons`) matching VS Code's monochrome status bar conventions (`rust-analyzer` / `Copilot` / `Biome` style), paired with `$(error)` / `$(warning)` on findings and `$(circle-slash)` when linting is off.

## 0.4.0

- **Status Bar Control Center & Configurable Lint Cadence**: Added context-aware clickable `svlang` status bar item + Quick Pick menu (`Lint Now`, `Lint Trigger: onType / onSave / off`, `Switch Preset`, `Set Preprocessor Defines`, `Initialize / Open Project Config`, `Settings`, `Output`, `Restart`).
- **Compiler-Backed Semantic Highlighting & `` `ifdef `` Dimming**: Added `textDocument/semanticTokens/full` and configurable dimming of untaken `` `ifdef `` / `` `ifndef `` / `` `else `` conditional blocks (`svlang.inactiveRegions.*`), updating live with `svlang.compile.defines`.
- **RTL Hierarchy, Signal Driver/Load Tracing & Type/Call Hierarchies**: Added **Explorer → RTL Hierarchy** and **Explorer → Signal Trace** (separating Drivers/Writes from Loads/Reads via right-click `Trace Signal Drivers & Loads`), plus LSP `TypeHierarchy` (UVM/SV class inheritance) and `CallHierarchy`.
- **Rich 3-Layer Configuration & One-Click Rule Ignore**: Added categorized Settings UI (`svlang.lint.{requireConfig, configPath, configPreference, select, ignore}`, `svlang.compile.{filelists, defines, includeDirs}`, `svlang.files.extraExtensions`), `.f`/`.vc` filelist syntax support, `source.fixAll.svlang` on-save autofix, and `Cmd+.` quick actions to ignore rules in Workspace or User settings.
- **Refined Atom-Material File Icons & Rule Precision Fixes**: Updated `.sv`/`.v` file icons to airy die-corner `SV`/`V` icons (`M5`) and eliminated standalone-mode false positives in `SV-OOP-011` (`uvm_bitstream_t`), `SV-DED-006` (queue/collection mutations), `UVM-BAN-001/002` (comments/strings), `SV-DED-001`, and `SEC-LOG-001`.

## 0.3.0

- **Unified `svlang` CLI & LSP**: Consolidated the bundled binary into `svlang` (`svlang lsp`), eliminating multi-binary packaging and retiring the legacy C++ engine.

## 0.2.1

- Refined 256×256 silicon die Marketplace icon and `.sv` / `.v` file tab icons.

## 0.2.0

- Initial public release under `svlang.svlang` across Open VSX and VS Code Marketplace (`linux-x64`, `linux-arm64`, `darwin-arm64`, `darwin-x64`, `win32-x64`).

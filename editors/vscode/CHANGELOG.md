# Changelog

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

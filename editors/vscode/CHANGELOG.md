# Changelog

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

# Changelog

All notable changes to the **svlang** VS Code / Open VSX extension (`svlang.svlang`) are documented here.

## [0.2.0] - 2026-11-20

### Added
- Initial release under the `svlang.svlang` extension ID with bundled native Rust `dv-lsp` across `linux-x64`, `linux-arm64`, `darwin-arm64`, `darwin-x64`, and `win32-x64`.
- Live diagnostics and quick fixes across 446 IEEE 1800-2023 SystemVerilog, UVM, RTL/FSM/CDC/dataflow, SVA, coverage, and style rules, with Verissimo-compatible scoped block waivers (`svlang-lint:off`/`on`, `verissimo_lint:*`).
- `svlang.preset` setting forwarding to `dv-lsp` (`initializationOptions.preset`), including the `formal-ready` pre-FPV rule pack (`SV-FPV-001..005`) and quick fixes.
- Semantic code browsing (outline, workspace symbols, go to definition/declaration/type definition, references, hover) and context-aware completion.
- Built-in SystemVerilog/Verilog TextMate grammars and UVM snippets.
- Single-file `esbuild` extension bundle for fast cold activation and minimal `.vsix` footprint.

# svlang — SystemVerilog & UVM

Pure-Rust **IEEE 1800-2023 SystemVerilog** and **IEEE 1800.2 UVM** language server, compiler-backed semantic highlighter, RTL hierarchy & signal tracer, and 475-rule static linter (`svlang lsp`) for VS Code, Cursor, and Antigravity.

Bundles a self-contained native `svlang` binary — **zero C++, Python, Java, or simulator setup required**.

---

## Quick Start

1. **Open any `.sv`, `.svh`, `.v`, `.vh`, or `.f` file**:
   - **Code browsing & live compiler checks start immediately (Mode 1 default)**: semantic highlighting, inactive `` `ifdef `` dimming, hover docs, completion, go-to-definition (`F12`), find references (`Shift+F12`), and live IEEE 1800-2023 syntax/semantic errors (`CMP-ERR-001`) work out of the box while static lint rules stay quiet until requested.
2. **Click `svlang` in the Status Bar** (bottom-right) to open the **Control Center**:
   - **Lint Current File / Project Now** — run static lint rules on demand.
   - **Set Lint Trigger** — keep `Off` (Mode 1 default), or switch to `On Save` / `On Type`.
   - **Switch Lint Preset** — `default`, `formal-ready`, `soundness`, `methodology`, `style`, `all`, …
   - **Set Preprocessor Defines** — live-updates `` `ifdef `` dimming and elaboration.
   - **Initialize Project Config** — runs `svlang lint --init` to auto-discover `.f` filelists, `$UVM_HOME`, and naming conventions into `.svlang-lint/config.py`.
3. **Explore RTL & UVM structure**:
   - **Trace Signal Drivers & Loads**: right-click any signal, port, variable, or class field → **`svlang: Trace Signal Drivers & Loads`** (opens **Explorer → Signal Trace** separating **Drivers / Writes** from **Loads / Reads**).
   - **RTL Hierarchy**: open **Explorer → RTL Hierarchy** to navigate your elaborated module/interface instantiation tree.
   - **Type & Call Hierarchy**: right-click any UVM/SV class or routine → **Show Type Hierarchy** / **Show Call Hierarchy**.

---

## Key Capabilities

- **Compiler-Backed Highlighting & `` `ifdef `` Dimming**: True semantic tokens (`class`, `interface`, `namespace`, `type`, `enumMember`, `parameter`, `property`, `method`, `macro`) plus configurable dimming of untaken `` `ifdef `` / `` `ifndef `` / `` `elsif `` / `` `else `` regions.
- **Structured `DOM-GRP-NNN` (`3-3-3`) Linter (475 Rules)**: Organized across 5 EDA domains (`CMP`, `RTL`, `SVL`, `SVA`, `UVM`) and 34 3-letter groups with prefix wildcard selection and ignoring (`"RTL-*"`, `"SVL-CON-*"`, `"UVM-NAM-*"`).
- **One-Click Quick Fixes (`Cmd+.` / `Ctrl+.`)**:
  - Apply simulation-safe rule rewrites (or fix all safe issues on save via `"editor.codeActionsOnSave": { "source.fixAll.svlang": "explicit" }`).
  - Insert inline waivers (`// svlang-lint-disable-next-line <RULE>`).
  - **Ignore `<RULE>` in Workspace or User Settings** in one click.
- **Simulator `.f` Filelist Support**: Syntax highlighting for `.f` / `.F` / `.vc` command files, `$ENV_VAR` expansion, and zero-boilerplate `svlang.compile.filelists` wiring.

---

## Lint Rule Domains & Presets (`DOM-GRP-NNN`)

Every diagnostic uses a 3-letter Domain, 3-letter Group, and 3-digit Number (`DOM-GRP-NNN`), and both `svlang.lint.select` and `svlang.lint.ignore` accept `*` prefix wildcards:

| Selector | Domain | Key Groups | What It Catches |
| :--- | :--- | :--- | :--- |
| `CMP-*` | **Compiler** | `CMP-ERR` | Preprocessor, syntax, elaboration, and IEEE 1800 semantic errors (`CMP-ERR-001`) |
| `RTL-*` | **RTL Design** | `RTL-SYN`, `RTL-FSM`, `RTL-CDC`, `RTL-DAT` | Reset polarity/partial resets, gated clocks, inferred latches, clocked blocking races, FSM deadlocks/unreachable states, CDC, `casex`/X-optimism, reversed part-selects |
| `SVL-*` | **SystemVerilog** | `SVL-COR`, `SVL-TYP`, `SVL-OOP`, `SVL-CON`, `SVL-RND`, `SVL-COV`, `SVL-PRF`, `SVL-DED`, `SVL-MNT`, `SVL-PRT`, `SVL-DPI`, `SVL-SEC`, `SVL-DEP`, `SVL-STY`, `SVL-NAM`, `SVL-WVR` | Expression/CFG bugs, signed/unsigned & enum casts, OOP/`$cast`, `fork`/`join` & clocking races, unsatisfiable constraints, covergroups, performance, dead code, smells, DPI-C, style & naming |
| `SVA-*` | **Assertions & Formal** | `SVA-PRP`, `SVA-FPV` | SVA clocks/resets/action blocks, vacuous antecedents, and pre-FPV formal readiness (`SVA-FPV-001..005`) |
| `UVM-*` | **IEEE 1800.2 UVM** | `UVM-FAC`, `UVM-PHS`, `UVM-OBJ`, `UVM-CFG`, `UVM-SEQ`, `UVM-TLM`, `UVM-CMP`, `UVM-RAL`, `UVM-DEP`, `UVM-PRT`, `UVM-NAM` | Factory, phasing, inter-procedural objection CFG, `uvm_config_db`, sequences, TLM/scoreboards, component roles, RAL register maps, UVM deprecations & naming |

### Recommended `.vscode/settings.json` Recipes

**1. RTL Design & Formal Signoff (lint RTL + SVA on save):**
```json
{
  "svlang.lint.trigger": "onSave",
  "svlang.compile.filelists": ["rtl/files.f"],
  "svlang.compile.defines": ["SYNTHESIS"],
  "svlang.lint.select": ["RTL-*", "SVA-*"]
}
```

**2. UVM Verification Workspace (lint on save, silence naming conventions):**
```json
{
  "svlang.lint.trigger": "onSave",
  "svlang.preset": "default",
  "svlang.lint.ignore": ["SVL-NAM-*", "UVM-NAM-*"]
}
```

**3. Quiet Browsing + Syntax/Semantics Only (Built-In Default — Mode 1):**
```json
{
  "svlang.lint.trigger": "off",
  "svlang.diagnostics.syntaxAndSemantics": true
}
```

---

## Inline & Project Waivers

Suppress individual findings in source comments or centrally in `.svlang-lint/config.py`:

```systemverilog
// Same-line or next-line waiver (exact ID or prefix wildcard, optional message regex)
if (a == a) check(); // svlang-lint:waive SVL-COR-004 -- intentional self-test
// svlang-lint-disable-next-line UVM-OBJ-002
phase.raise_objection(this, "stress iteration");

// Scoped block waiver (Verissimo `waive_start` / `waive_stop` also supported)
// svlang-lint:off SVL-STY-*
`include "legacy_vendor_defs.svh"
// svlang-lint:on SVL-STY-*
```

---

## Configuration Precedence (Repo vs. Workspace vs. User)

1. **Repo `.svlang-lint/config.py` (Team & CI Truth)**: Wins by default (`svlang.lint.configPreference = "filesystemFirst"`) so editor diagnostics match `svlang lint` in CI 1:1.
2. **Workspace `.vscode/settings.json`**: Configure `svlang.compile.{filelists,defines,includeDirs}`, `svlang.lint.trigger`, or `svlang.preset` when no repo config is checked in (or set `configPreference` to `"editorFirst"` to override repo configs locally).
3. **User `settings.json`**: Personal defaults across all projects (`svlang.lint.requireConfig`, `svlang.inactiveRegions.opacity`, `svlang.statusBar.show`).

### Settings Summary

| Setting | Default | Description |
| :--- | :--- | :--- |
| `svlang.enable` | `true` | Master switch for diagnostics (navigation & highlighting stay active when `false`) |
| `svlang.diagnostics.syntaxAndSemantics` | `true` | Live IEEE 1800-2023 syntax & semantic checks (`CMP-ERR-001`) when `lint.trigger` is `"off"` |
| `svlang.lint.trigger` | `"off"` | `"off"` (Mode 1: browsing + syntax/semantics; manual `Lint Now`), `"onSave"`, or `"onType"` |
| `svlang.lint.requireConfig` | `false` | Only auto-lint workspaces containing `.svlang-lint/config.py` (or `lint.configPath`) |
| `svlang.lint.configPath` | `""` | Shared/fallback `.svlang-lint/config.py` path (`~` and `$VARS` expanded) |
| `svlang.lint.configPreference` | `"filesystemFirst"` | `"filesystemFirst"` (repo config wins) or `"editorFirst"` (VS Code settings win) |
| `svlang.preset` | `""` | Rule preset (`default`, `formal-ready`, `soundness`, `methodology`, `style`, `all`, …) |
| `svlang.lint.select` | `[]` | Extra rule IDs, names, or `DOM-GRP-*` wildcards to enable |
| `svlang.lint.ignore` | `[]` | Rule IDs, names, or `DOM-GRP-*` wildcards to silence |
| `svlang.compile.filelists` | `[]` | Simulator `.f` command files when no `.svlang-lint/config.py` exists |
| `svlang.compile.defines` | `[]` | Preprocessor defines (`["SYNTHESIS", "WIDTH=64"]`) for lint & `` `ifdef `` dimming |
| `svlang.compile.includeDirs` | `[]` | Extra `+incdir+` directories (`~` and `$VARS` expanded) |
| `svlang.files.extraExtensions` | `[]` | Additional extensions treated as SystemVerilog (e.g. `[".sva", ".inc"]`) |
| `svlang.inactiveRegions.enable` | `true` | Dim untaken `` `ifdef `` / `` `ifndef `` / `` `else `` regions (`opacity`: `0.5`) |
| `svlang.navigation.enable` | `true` | Outline, workspace symbols, definitions, references, hierarchies, hover, completion |
| `svlang.semanticHighlighting.enable` | `true` | Compiler-backed LSP semantic token highlighting |
| `svlang.navigation.useUvmHome` | `true` | Index `$UVM_HOME/src` automatically when `UVM_HOME` is set |
| `svlang.serverPath` | `"svlang"` | Path to `svlang` executable (uses bundled platform binary by default, else `PATH`) |

---

## Supported Platforms & Links

Self-contained native `.vsix` bundles are published for:
- **Linux**: `linux-x64`, `linux-arm64` (glibc 2.28+)
- **macOS**: `darwin-arm64` (Apple Silicon), `darwin-x64`
- **Windows**: `win32-x64`

[GitHub Repository](https://github.com/mayurkubavat/svlang) · [Rule Catalog (`DOM-GRP-NNN`)](https://github.com/mayurkubavat/svlang/blob/main/docs/rules/README.md) · [Linter & Config Guide](https://github.com/mayurkubavat/svlang/blob/main/docs/guide/svlang-lint.md) · [Issue Tracker](https://github.com/mayurkubavat/svlang/issues)

## License

Apache-2.0 — see `LICENSE.txt` and `NOTICE`.

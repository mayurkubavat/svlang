# Changelog

What changed in each release of the svlang crates (`svlang`, `sv-*`,
`svlang-model`, `svlang-lint`, `svlang-lsp`, `svlang-cli`), `pysvlang`, and the
`svlang.svlang` VS Code / Open VSX extension. They share one version number.

## 0.5.1 (2026-11-22)

### Elaboration Fixes (`sv-semantic`, `sv-expr`, `sv-ir`)

Found by converting 2,184 modules of open-source RTL with `svcc` and comparing
the result against a simulator. Each one changed the elaborated design, and so
the netlist, without a diagnostic.

- **Generate condition with a reduction operator** (27.5, 11.4.9):
  `if (|P) ... else ...` elaborated neither branch, because the elaboration-time
  evaluator had no `&`, `|`, `^` (and negated) prefix operators. It now takes
  the branch the value selects.
- **Fill literal as the default of a sized parameter** (5.7.1, 6.20.2):
  `parameter logic [W-1:0] P = '1` had the value 1 in the elaborated design
  (the printed hierarchy was already right). `'0`, `'1`, `'x` and `'z` now set
  every bit of the context they are sized by, also inside `?:` and operators.
- **Select on a packed struct member with its own range** (7.2.1, 11.5.1): for
  `logic [23:16] m`, `s.m[22:16]` was indexed against `[7:0]` and lowered as
  out of range (x). `PackedField` gains `bounds`, the member's declared range,
  and selects use it. This adds a public field to `sv_ir::PackedField`.

## 0.5.0 (2026-11-22)

### Compiler Front-End Fixes (`sv-lexer`, `sv-syntax`, `sv-semantic`)

Found by running `svlang check` and an event-driven simulator's front end over
the same 1,102 SystemVerilog test files and reading IEEE 1800-2023 for every
disagreement.

- **Legal code that was rejected, now accepted**: UDP instances with no
  instance name, including the drive-strength and delay forms (29.8);
  transition bins with a repetition and no `=>`, such as `(5 [*2])` (19.5.2);
  a constraint expression that starts with a concatenation,
  `{a, b} == {c, d};` (18.5); a based literal whose digits follow a newline
  (5.7.1); `default clocking <name>;` naming an existing block, previously a
  redefinition error (14.12); implicit generate block names such as
  `genblk2[0].genblk1[0]` (27.6); and a later standard's keyword used as a
  plain name under `` `begin_keywords "1364-2005" `` (22.14).
- **Illegal code that was accepted, now reported**: a module instance with no
  instance name (23.3.2); `import p::x;` of a name package `p` does not
  declare (26.3); and an assignment to a `const` variable outside a class
  (6.20.6).

### Workspace Layout (`sv-*` crates)

- **Seven crates extracted from `sv-semantic`**: `sv-bigint` (arbitrary-width
  integer), `sv-fxhash` (hasher), `sv-scopes` (scopes and name checks),
  `sv-ir` (elaborated-design types, constant values, source positions),
  `sv-checks` (syntax-level legality checks), `sv-expr` (typed expression
  binder), and `sv-sva` (property and sequence binder). `sv-semantic`
  re-exports every item at its previous path, so code that uses `svlang` or
  `sv_semantic` needs no change. `sv-semantic` itself is now the elaboration
  core (32.6K -> 14.7K lines).
- **Internal items made reachable across the new crate lines**: 14 helper
  functions and constants and 4 struct fields are now `pub` with
  `#[doc(hidden)]`. They are not part of the supported API.
- **Internal crates declared once** in the root `[workspace.dependencies]`;
  `tools/release/publish_crates.sh --dry-run` now fails when an internal
  dependency's version does not match the workspace version.

## 0.4.0 (2026-11-21)

### IDE / LSP Highlighting, Code Tracing & Control Center (`svlang-lsp` + `svlang.svlang`)

- **Semantic Highlighting & Inactive `` `ifdef `` Dimming**: implemented
  `textDocument/semanticTokens/full` (`namespace`, `class`, `interface`, `type`,
  `enumMember`, `parameter`, `variable`, `property`, `function`, `method`,
  `macro` + `declaration`, `readonly`, `static`, `modification` modifiers) and
  `svlang/inactiveRanges` dimming untaken `` `ifdef `` / `` `ifndef `` /
  `` `elsif `` / `` `else `` regions (`svlang.inactiveRegions.*`), updating live
  when `svlang.compile.defines` changes.
- **RTL Hierarchy, Signal Driver/Load Tracing & Type/Call Hierarchies**:
  implemented `svlang/rtlHierarchy` (workspace module/interface instantiation
  tree), `svlang/traceSignal` (separating procedural, continuous, and
  output-port **Drivers / Writes** from **Loads / Reads**),
  `textDocument/prepareTypeHierarchy` (`supertypes` / `subtypes`), and
  `textDocument/prepareCallHierarchy` (`incomingCalls` / `outgoingCalls`), plus
  Explorer `RTL Hierarchy` and `Signal Trace` sidebar views.
- **Status Bar Control Center & 3-Layer Configuration**: added a context-aware
  clickable `svlang` status bar item + Quick Pick control menu, dynamic
  live-reconfiguration (`svlang/setLintOptions`, `svlang/lintNow`,
  `svlang.lint.trigger`: `onType` / `onSave` / `off`), `EditorOptions`
  (`svlang_lint::EditorOptions`: `requireConfig`, `configPath`,
  `configPreference` `filesystemFirst` vs `editorFirst`, `select`, `ignore`,
  `filelists`, `defines`, `includeDirs`), `source.fixAll.svlang` on-save
  autofix, `.f`/`.vc` filelist syntax support, one-click `Ignore <RULE>` code
  actions, and Atom-Material-inspired die-corner `.sv`/`.v` file icons.

### Linter Precision Fixes (`svlang-lint`)

- **`SV-OOP-011` (`DowncastWithoutDynamicCast`)**: eliminated false positives on
  uncompiled UVM integral/enum typedef casts (such as `uvm_bitstream_t'(1)`) by
  consulting the embedded UVM class API pack (`uvm_api`), excluding value/enum
  typedef suffixes (`_t`, `_e`, `_enum`, `_s`, `_u`), and skipping arithmetic /
  literal / concatenation operands.
- **`SV-DED-006`, `UVM-BAN-001/002`, `SV-DED-001`, `SEC-LOG-001`**: recognized
  mutating queue/collection/IPC method calls (`push_back`, `insert`, `delete`,
  `pop_front`, `put`, …) and indexed LHS roots as property writes in
  `SV-DED-006`; switched `UVM-BAN-001/002` to lexer `Directive` tokens so
  comments and strings never trigger; added syntactic identifier fallback in
  `SV-DED-001` when external imports are uncompiled; and trimmed trivia +
  excluded generic `.key` struct lookups in `SEC-LOG-001`.

## 0.3.0 (2026-11-20)

### Unified `svlang` CLI & `svlang-*` Crate Family

- **Single unified `svlang` binary (`svlang-cli`)**: consolidated all native
  tools (`svlang`, `svlang-lint` / `dv-lint`, `dv-lsp`, `dv-build`,
  `dv-filelist`, `dv-map`) into one zero-async-bloat pure-Rust binary:
  - `svlang check` (with oracle/gate inspection flags replacing `dv-build`)
  - `svlang lint` (static SV/UVM/FPV linter & `--fix`, plus `argv[0]`
    auto-dispatch when symlinked as `svlang-lint`)
  - `svlang lsp` (in-process Language Server Protocol server replacing `dv-lsp`)
  - `svlang filelist` (VCS/Xcelium filelist doctor & normalizer replacing `dv-filelist`)
  - `svlang map` (UVM testbench component hierarchy replacing `dv-map`)
  - plus `preprocess`, `hier`, `cst`, `netlist`, `btor2`, `aiger`, `yosys`, `stages`
- **Retired `dv-*` crate names & made all 14 pure-Rust crates publishable**:
  renamed `dv-model` -> **`svlang-model`**, `dv-lint` -> **`svlang-lint`**,
  `dv-lsp` -> **`svlang-lsp`**, removed `dv-build`/`dv-filelist`/`dv-map`,
  inverted the internal `sv-slang` oracle dependency so zero publishable crates
  reference unpublished C++ bindings, and embedded `crates/svlang-lint/data/`
  directly inside `svlang-lint`.
- **Deliverables**: release archives ship as `svlang-<target>.tar.gz` (one
  `bin/svlang[.exe]` binary + Python SDK); VS Code / Open VSX `svlang.svlang`
  `0.3.0` `.vsix` packages bundle `svlang[.exe]` and launch `svlang lsp` with
  automatic backwards-compatibility for existing `"dv-lsp"` user settings.
- **Retired legacy C++20 linter (`cpp/` + `crates/sv-slang/`)**: deleted the
  51k-LOC C++ linter engine, C ABI FFI bridge, and C++ CI/release jobs;
  migrated end-to-end Python suites (`tests/test_*.py`) and nightly precision
  monitoring directly onto Rust `svlang lint`. Retained only the tiny (~880 LOC)
  opt-in Slang compiler differential dumpers (`tools/oracles/`, `cmake --preset oracles`)
  for IEEE 1800 conformance gates.

## 0.2.0 (2026-11-19)

### New crates and tools

- **`sv-aiger`**: the netlist bit-blasted to an and-inverter graph and
  written as AIGER 1.9, binary or text, with names in the symbol table.
  Assertions are bad states, assumptions are constraints, and liveness
  obligations are justice properties with fairness constraints.
- **`sv-yosys`**: the netlist as a Yosys JSON netlist built from Yosys's own
  word-level cells, for synthesis, equivalence checking and SymbiYosys. The
  tests have Yosys prove the output equivalent to its own reading of the same
  SystemVerilog.
- **`svlang-cli`**: the `svlang` command: `check` (text or JSON), `preprocess`,
  `hier`, `cst`, `netlist`, `btor2`, `aiger`, `yosys` and `stages`, over the
  compile arguments a simulator takes.
- **`svlang-lint` 0.2.0**: unified native SystemVerilog and UVM linter, language
  server (`dv-lsp`), and Python configuration/rule SDK (`svlang_lint`) with
  thread-safe `dv_lint::Session`, Preprocessor Event Table + `TreeLayout`,
  scoped block waivers, and 441 rules (469 catalog entries).
- **`svlang-wasm`** (in the repository, not published): svlang as a 2.5 MB
  WebAssembly module with a playground page.

### Formal

- Concurrent properties are compiled into checks
  (`Netlist::compile_properties`): bounded and unbounded sequences (`[->m:n]`,
  `[=m:n]`, `##[m:$]`, `[*m:$]`), `|->` and `|=>`, `#-#` and `#=#` followed-by,
  property negation `not`, `accept_on` / `reject_on` / `sync_accept_on` /
  `sync_reject_on` aborts, `disable iff`, `until` and `s_until`, `strong(...)`,
  `nexttime`, `eventually`, `always`, and `s_eventually` liveness obligations
  via Glushkov/Thompson NFAs and ROBDD DFAs. Properties can also be bound from
  text after lowering, with optional `:precondition` vacuity and `:witness`
  covers (`PropertyOptions::{precondition_covers, witness_covers}`) and
  `$anyseq` / `$anyconst` / `$allseq` / `$allconst` formal free values.
- The Btor2 writer takes designs it used to refuse: latches, both edges of
  a clock, several declared clocks, clocks computed in the design, and a
  reset held for the first steps. Covers can be written as reachable bad
  states.
- Opt-in lowering switches for two-state tools: `skip_initial_checks` and
  `isunknown_is_false`.
- The Btor2 writer declares an initial value before its state, which the
  reference parser of btor2tools requires.

### Front end

- IEEE 1800-2023 elaboration and lowering across Clause 17 `checker`s (with
  free `rand` variables), Clauses 28–31 `primitive`/UDP + tri-state/pull/array
  gates + `specify` path/timing checks, Clause 33 `config` blocks, Clause 6.6.7/
  6.6.8 `nettype` & `interconnect`, `bind` directives (including `genvar`
  scopes), and Clause 11.4.14 `{<<}` / `{>>}` streaming concatenations.
- IEEE 1800-2023 OOP and semantic checks: `:final` classes, `:initial` /
  `:extends` / `:final` methods and constraints, `extends Base(default)` and
  `super.new(default)` constructor forwarding, `weak_reference`, `covergroup
  extends`, restricted type parameters (`parameter type struct/enum/class`),
  soft packed unions (`union soft packed`), triple-quoted string literals,
  opt-in per-file macro isolation (`Driver::separate_units` /
  `--separate-units`), and unified `ExprBinder` static typing (`0` mismatches
  across 99,712 expressions on `expr_gate.py`).
- A variable with more than one driver is an error (IEEE 1800 6.5).
- A `-top` that names no module is an error; it used to give an empty design.
- An `` `include `` named by a macro no longer loses the token after it, and
  macro text that splits a string literal is an error (22.5.1).
- Three ways the parser or type printer could loop forever or panic on text
  that is not SystemVerilog are fixed, along with mutual parameter/function
  return-dimension cycles.
- Many constructs found by the census of open RTL now lower: check-only
  `initial` blocks, end labels, parameter tables, type parameters in casts,
  package tables, wide structs, streaming concatenation of packed values,
  powers of two, labelled and two-armed asynchronous reset blocks, time
  literals in parameters, `$bits` on unpacked structs/arrays, and more.
  Unknown module instances are errors, not silent holes.
- Enum `next()` and `prev()`, and a variable in scope in its own initializer
  (`e_t t = t.first()` in a `for` loop).
- A part-select written against the declared direction (`a[0:3]` of
  `logic [3:0] a`) is an error (11.5.1); it used to be read as `a[3:0]`.
- A connection to a port the module does not have, and an override of a
  parameter it does not have, are errors; both used to be ignored.
- A member label in an assignment pattern is not a use before declaration.
- A macro may be used inside its own argument.
- An attribute written through a macro (`` `define KEEP (* keep *) ``) is
  recognised.

### Python (`pysvlang`)

- `Netlist.to_aiger()` and `Netlist.to_yosys_json()`; `to_btor2()` gains
  `covers`, `both_edges` and `clocks`.
- `Compilation(..., separate_units=True)` and
  `lower_netlist(compile_properties=True, skip_cycles=N, precondition_covers=True, witness_covers=True, skip_initial_checks=True, isunknown_is_false=True)`.
- The netlist and the design as objects: `Netlist.value()`, `registers`,
  `memories`, `checks`, `liveness`, and `Compilation.scope_tree()`.

### Measured

- [sv-tests](docs/reference/sv-tests.md): all 1,024 tests svlang can judge
  pass.
- [Front-end census](docs/reference/census.md): how much of 15 open designs
  lowers to a netlist, and what blocks the rest.
- [Lowering speed](docs/benchmarks/lowering.md): stage times, memory and
  netlist size for the 1,081 census tops that lower, and `svlang check`
  against slang on the same input.
- `tools/quality/mutate_smoke.py`: tens of thousands of damaged source files,
  none of which may crash or hang the front end.

## 0.1.1 (2026-11-14)

- **`sv-btor2`**: the netlist as a Btor2 transition system for word-level
  model checkers; `svlang::btor2` and `Netlist.to_btor2()` in Python.
- The repository front page describes the svlang library.

## 0.1.0 (2026-11-14)

- First release on crates.io (`svlang`, `sv-source`, `sv-lexer`,
  `sv-preprocessor`, `sv-syntax`, `sv-semantic`, `sv-netlist`) and of
  `pysvlang` on PyPI, with wheels for Linux, macOS and Windows.

# Changelog

What changed in each release of the svlang crates (`svlang`, `sv-*`,
`svlang-cli`) and of `pysvlang`. They share one version number.

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

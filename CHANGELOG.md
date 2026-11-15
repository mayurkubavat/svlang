# Changelog

What changed in each release of the svlang crates (`svlang`, `sv-*`,
`svlang-cli`) and of `pysvlang`. They share one version number.

## Unreleased (0.1.2)

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
- **`svlang-wasm`** (in the repository, not published): svlang as a 2.5 MB
  WebAssembly module with a playground page.

### Formal

- Concurrent properties are compiled into checks
  (`Netlist::compile_properties`): bounded sequences, `|->` and `|=>`,
  `disable iff`, `until` and `s_until`, bounded `nexttime`, `eventually` and
  `always`, and `s_eventually` as a liveness obligation. Properties can also
  be bound from text after lowering.
- The Btor2 writer takes designs it used to refuse: latches, both edges of
  a clock, several declared clocks, clocks computed in the design, and a
  reset held for the first steps. Covers can be written as reachable bad
  states.
- Opt-in lowering switches for two-state tools: `skip_initial_checks` and
  `isunknown_is_false`.

### Front end

- A variable with more than one driver is an error (IEEE 1800 6.5).
- A `-top` that names no module is an error; it used to give an empty design.
- An `` `include `` named by a macro no longer loses the token after it, and
  macro text that splits a string literal is an error (22.5.1).
- Three ways the parser or type printer could loop forever or panic on text
  that is not SystemVerilog are fixed.
- Many constructs found by the census of open RTL now lower: check-only
  `initial` blocks, end labels, parameter tables, type parameters in casts,
  package tables, wide structs, streaming concatenation of packed values,
  powers of two, labelled and two-armed asynchronous reset blocks, and more.
  Unknown module instances are errors, not silent holes.

### Python (`pysvlang`)

- `Netlist.to_aiger()` and `Netlist.to_yosys_json()`; `to_btor2()` gains
  `covers`, `both_edges` and `clocks`.
- `lower_netlist(compile_properties=True, skip_cycles=N)`.
- The netlist and the design as objects: `Netlist.value()`, `registers`,
  `memories`, `checks`, `liveness`, and `Compilation.scope_tree()`.

### Measured

- [sv-tests](docs/reference/sv-tests.md): all 1,024 tests svlang can judge
  pass.
- [Front-end census](docs/reference/census.md): how much of 15 open designs
  lowers to a netlist, and what blocks the rest.
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

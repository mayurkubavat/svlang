# `sv-yosys`

Writes the [svlang](https://crates.io/crates/svlang) RTL netlist in the JSON
netlist format of [Yosys](https://yosyshq.net/yosys/), using Yosys's own
word-level cells (`$add`, `$mux`, `$dff`, ...). A design read by svlang can
then go through anything Yosys offers: synthesis, technology mapping,
equivalence checking, netlist drawing, SymbiYosys. Pure Rust, no
dependencies outside svlang.

```rust
use svlang::{Compilation, LowerConfig};

let comp = Compilation::from_args(["rtl/counter.sv", "-top", "counter"])?;
let cfg = LowerConfig { blast_memories_up_to: Some(1024), ..Default::default() };
let netlist = comp.lower_netlist(&cfg).unwrap_netlist();
let model = svlang::yosys::write(&netlist, &Default::default())?;
std::fs::write("counter.json", model.json)?;
```

```sh
yosys -p 'read_json counter.json; synth; stat'
```

## What is written

The netlist is flat, so the file holds one module: the top module, with its
ports.

- Every operator is the Yosys cell of the same meaning. Slices,
  concatenations and extensions are wiring and make no cell.
- A register is a `$dff`; with an asynchronous reset an `$adff` (constant
  reset value) or `$aldff` (any value); a latch is a `$dlatch` or `$adlatch`.
  An initial value is the `init` attribute of the register's wire.
- `$past` and the other sampled-value functions are registers on their
  clock.
- `(* anyconst *)` is `$anyconst`; `(* anyseq *)`, cut points and black-box
  outputs are `$anyseq`. An undriven bit is left undriven, and an X or Z
  constant bit is the constant `x` or `z`.
- A check is a `$assert`, `$assume` or `$cover` cell named by its label.
  Concurrent properties are written through the checks
  `Netlist::compile_properties` made; the rest are left out and
  `Model::notes` says how many. `Options::checks` turns the cells off for a
  synthesis flow.
- Registers and the design's named signals are wires under their
  hierarchical names (`top.u0.count`), and each cell carries its source
  location as the `src` attribute.

Memories must be expanded into registers first
(`LowerConfig::blast_memories_up_to`). What has no exact form here
(`$isunknown`, a power with a non-constant exponent) is an error, never an
approximation.

## How it is checked

The test suite asks Yosys to prove the JSON equivalent to Yosys's own reading
of the same SystemVerilog, for designs that cover the operators, signed
arithmetic, shifts, every kind of register, latches, hierarchy and memories,
and checks that the proof fails when a cell is swapped for another. It runs
when Yosys is installed (`YOSYS=/path/to/yosys cargo test -p sv-yosys`).

## License

Apache-2.0.

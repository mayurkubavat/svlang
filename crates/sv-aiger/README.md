# `sv-aiger`

Bit-blasts the [svlang](https://crates.io/crates/svlang) RTL netlist to an
and-inverter graph and writes it as [AIGER](https://fmv.jku.at/aiger/) 1.9,
the input format of bit-level model checkers and logic tools such as rIC3,
ABC, nuXmv and the HWMCC solvers. Pure Rust, no dependencies outside svlang.

```rust
use svlang::{Compilation, LowerConfig};

let comp = Compilation::from_args(["rtl/counter.sv", "-top", "counter"])?;
let cfg = LowerConfig { blast_memories_up_to: Some(1024), ..Default::default() };
let netlist = comp.lower_netlist(&cfg).unwrap_netlist();
let model = svlang::aiger::lower(&netlist, &Default::default())?;
std::fs::write("counter.aig", model.aig.to_binary())?;   // or to_ascii() for .aag
```

Then, for example, `ric3 check counter.aig ic3`.

## What the model means

The graph is the Btor2 model of [`sv-btor2`](https://crates.io/crates/sv-btor2),
bit by bit:

- One step is one clock edge of a single clock; two clocks are an error. A
  SystemVerilog latch is transparent within a step, as in `sv-btor2`. An
  asynchronous reset acts synchronously.
- Every register bit is a latch, with its constant initial value or none.
  Every input bit, and every free bit (undriven, X, `anyseq`, cut point,
  black-box output), is an input.
- An immediate `assert` is a bad state (`B` section); an immediate `assume`
  is an invariant constraint (`C` section).
- Inputs, latches and bad states are named in the symbol table
  (`i0 top.rst`, `l3 top.q[3]`, `b0 my_label`).
- Memories must be expanded into registers first
  (`LowerConfig::blast_memories_up_to`); arrays are an error.
- Concurrent assertions (`assert property`) are not written, and a
  concurrent assumption is an error, as in `sv-btor2`.

The crate also reads AIGER (`aiger::read`) and bit-blasts Btor2 text that
uses only bit-vectors (`from_btor2`).

## License

Apache-2.0.

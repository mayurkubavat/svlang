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
bit by bit, and `lower` takes that crate's options (covers, both clock edges,
declared clocks):

- Every register bit is a latch, with its constant initial value or none.
  Every input bit, and every free bit (undriven, X, `anyseq`, cut point,
  black-box output), is an input.
- An assertion is a bad state (`B` section) and an assumption is an
  invariant constraint (`C` section). Concurrent properties
  (`assert property`) are written through the checks
  `Netlist::compile_properties` made for them; see `sv-btor2` for which
  properties that covers and what happens to the rest.
- A liveness obligation (`req |-> s_eventually gnt`, compiled by
  `Netlist::compile_properties`) is a justice property (`J` section), and a
  liveness assumption a fairness constraint (`F` section).
- Inputs, latches, bad states and justice properties are named in the symbol
  table (`i0 top.rst`, `l3 top.q[3]`, `b0 my_label`, `j0 top.granted`).
- Memories must be expanded into registers first
  (`LowerConfig::blast_memories_up_to`); arrays are an error.
- Clocks, edges, latches and asynchronous resets are modelled as in
  `sv-btor2`; what that crate refuses is an error here too.

The crate also reads AIGER (`aiger::read`) and bit-blasts Btor2 text that
uses only bit-vectors (`from_btor2`).

## License

Apache-2.0.

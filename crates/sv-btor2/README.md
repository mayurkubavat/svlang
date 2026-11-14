# `sv-btor2`

Writes the [svlang](https://crates.io/crates/svlang) word-level RTL netlist
as a [Btor2](https://github.com/Boolector/btor2tools) transition system, the
input format of word-level model checkers such as rIC3, Pono, AVR and
BtorMC. Pure Rust, no dependencies outside svlang.

```rust
use svlang::{Compilation, LowerConfig};

let comp = Compilation::from_args(["rtl/counter.sv", "-top", "counter"])?;
let netlist = comp.lower_netlist(&LowerConfig::default()).unwrap_netlist();
let model = svlang::btor2::write(&netlist, &Default::default())?;
std::fs::write("counter.btor2", model.btor2)?;
for note in model.notes {
    eprintln!("note: {note}");
}
```

Then, for example, `ric3 check counter.btor2 ic3`.

## What the model means

- One step is one clock edge. Every register, memory write port and clocked
  check must use the same clock signal and edge; a design with two clocks,
  or with a latch, is rejected.
- An asynchronous reset acts synchronously.
- `(* anyconst *)` signals hold one free value for the whole trace.
  `(* anyseq *)` signals, cut points, black-box outputs, undriven bits and
  X or Z constant bits are free on every step.
- An immediate `assert` is a `bad` line named by its label; an immediate
  `assume` is a `constraint`.
- Concurrent assertions (`assert property`) and covers are **not** written.
  They stay in `Netlist::properties` for a tool that builds its own monitor,
  and `Model::notes` says how many were left out.
- A concurrent assumption (`assume property`, `restrict property`) cannot
  be left out without changing what the assertions mean, so a design that
  has one is an error for now.

Anything the model cannot express exactly is an error (`Fail`), never an
approximation.

## License

Apache-2.0.

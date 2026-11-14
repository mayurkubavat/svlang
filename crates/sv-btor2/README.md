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
  check must use the same clock signal and edge; a design with two clocks
  is rejected.
- A latch is transparent within a step: its value is its data where the
  data assigns it, and what it held at the end of the step before where
  the data leaves it alone. A latch whose value depends on itself within a
  step, through its own data or another latch, is rejected.
- With `Options::both_edges`, a design on both edges of one clock is
  written with two steps per clock cycle: the clock (a top-level input)
  starts low and toggles on every step, and each register, memory write,
  history and clocked check follows its own edge. Other inputs may change
  on every step. `Model::steps_per_cycle` says which model was written.
- With `Options::clocks`, a design on several clocks is written the same
  way: each clock in use is a top-level input declared with the steps it
  stays at one level and a phase. A clock computed in the design (divided
  or gated) is rejected.
- An asynchronous reset acts synchronously.
- `(* anyconst *)` signals hold one free value for the whole trace.
  `(* anyseq *)` signals, cut points, black-box outputs, undriven bits and
  X or Z constant bits are free on every step.
- An immediate `assert` is a `bad` line named by its label; an immediate
  `assume` is a `constraint`.
- A concurrent property (`assert property`, `assume property`) is written
  through the check `Netlist::compile_properties` made for it: a `bad` line
  under its full hierarchical name (`top.u0.a_no_overflow`), or a
  `constraint`. That pass compiles properties over bounded sequences
  (booleans, `##N`, `##[M:N]`, `[*N]`, `[*M:N]`, sequence `or`, `and`,
  `intersect`, `within`, `throughout`, `first_match`, under `|->`, `|=>` and
  `disable iff`); a property without a check is
  **not** written, and `Model::notes` says how many were left out.
- A cover is a `bad` line that is reached when the cover is hit, with
  `Options::covers`; without it covers are left out and noted.
- A concurrent assumption (`assume property`, `restrict property`) cannot
  be left out without changing what the assertions mean, so a design that
  has one without a check is an error.

Anything the model cannot express exactly is an error (`Fail`), never an
approximation.

## License

Apache-2.0.

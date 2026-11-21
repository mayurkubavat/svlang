# Semantic-analysis rules

This note highlights rules in `svlang lint` that perform deep semantic,
constraint, and cross-routine dataflow analyses over `svlang`'s front end and
`svlang-model`.

## Constraint, coverage, race, and API rules

| Rule | Name | Severity | What it flags |
|---|---|---|---|
| `UVM-API-003` | DeprecatedUvmApi | WARNING | Calls to UVM methods, and classes extending UVM classes, that the compiled UVM release keeps only as deprecated API (UVM 1.1d/1.2: what `UVM_NO_DEPRECATED` removes, such as `uvm_component::set_config_int` or `uvm_random_sequence`). Read from the release's API pack in `crates/svlang-lint/data/uvm-api/`. |
| `SV-RND-030` | ContradictoryConstraints | ERROR | A rand variable whose hard constraints leave no legal value (`len > 10` on a `bit [3:0]`; `x < 5` with `x inside {[8:9]}`). Bounds in different constraint blocks count together only when no user code calls `constraint_mode()`. |
| `SV-RND-031` | SoftConstraintNeverHolds | WARNING | A `soft` constraint the class's hard constraints always override. |
| `SV-RND-032` | DeadDistItem | WARNING | A `dist` item whose values the hard constraints or the type never allow, so its weight is dropped. |
| `SV-RND-040` | HardcodedBounds | WARNING | A non-`soft` literal constant bound on a `rand` property in a base class (`virtual` class or class extended in the design), which prevents derived classes from widening stimulus ranges without `constraint_mode(0)`. Advisory: off by default. |
| `SV-COV-018` | CrossBinCountTooLarge | WARNING | A cross whose bin count, computed as a simulator builds the bins (explicit, `[N]`, `[]`, automatic up to `auto_bin_max`), is above 10,000. |
| `SV-RCE-002` | ClockedBlockingRace | WARNING | A variable written with `=` in one edge-triggered always block and read by another on the same edge: a race (IEEE 1800 4.7). |
| `DPI-PRT-001` | ConflictingDpiPrototypes | ERROR | One C function imported with two different prototypes. |
| `SV-COV-019` | CovergroupNeverSampled | WARNING | A covergroup declared in a module, interface, program or package with no sampling event that nothing instantiates, or whose instances no `.sample()` call anywhere in the design targets (`cg_i.sample()`, `bus.cg_i.sample()`, `cg_q[i].sample(x)`). The class covergroups are SV-COV-001's. |
| `UVM-TLM-007` | UncoveredAnalysisPort | WARNING | A transaction type a monitor publishes on an analysis port that no coverage collector receives: no class with a covergroup (its own, a base class's or a held object's) extends `uvm_subscriber #(T)` or holds an analysis imp, export, FIFO or get/peek port of `T`. Only reported when the testbench declares covergroups at all; one finding per type, naming every port. |

The constraint analyzers only read bounds of one variable against constants
(comparisons, `inside`, `!`, `&&`, `soft`, `dist`) and leave everything else
out, so what they report is sound: if a subset of the constraints has no
solution, neither do all of them.

## Declaration-resolved interface rules

| Rule | Resolution behaviour |
|---|---|
| `SV-INT-001`, `SV-LANG-005` | An assignment target is an interface drive when its declarations say so: `cfg.bus.drv.data` with `bus` declared `virtual bus_if` (also through a typedef, an inherited property, or an array of handles) and `drv` a clocking block of `bus_if`. A struct called `my_vif` is not one. |
| `SV-DRV-001` | A task drives an interface when one of its `<=` targets is such a signal by declaration. |
| `UVM-VIF-DIR` | A monitor drives an interface when the assignment target is reached through a virtual interface handle, whatever it is called. |
| `SV-OOP-008` | A class member is a non-virtual interface when its type is a declared `interface`; an `interface class` called `printable_if` is not one. |

When a target or type cannot be resolved statically (e.g., a type parameter, a
call chain, or an uncompiled declaration), these rules fall back gracefully to
handle-naming heuristics.

## Cross-routine objection pairing

`UVM-OBJ-001`, `UVM-OBJ-003`, and `UVM-PHS-006` are answered from one shared
inter-procedural analysis (`svlang_lint::sem::objections`):

- A raise in one routine and matching drop in another routine of the class, a
  base class, or a derived class (`pre_body`/`post_body`,
  `pre_start`/`post_start`, helper tasks) are paired cleanly without false
  positives.
- Guarded pairs (`if (c) raise` ... `if (c) drop`, or `if (h != null) h.raise_objection(this)`)
  are tracked per handle and condition.
- Each objection handle (`phase` vs another handle) is tracked independently.
- Early `return` inside a loop while holding an objection, drops of unraised
  objections inside loops, and asymmetric branches are all detected accurately.

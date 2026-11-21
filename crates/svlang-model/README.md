# `svlang-model`

Safe, owned SystemVerilog, UVM, and RAL design model built on the pure-Rust
[`svlang`](https://crates.io/crates/svlang) front end.

It provides classified UVM classes (`uvm_test`, `uvm_env`, `uvm_agent`,
`uvm_driver`, `uvm_monitor`, `uvm_sequencer`, `uvm_scoreboard`, `uvm_sequence`,
`uvm_sequence_item`), phase methods, properties, calls, assignments, and UVM
register-abstraction-layer (`uvm_reg_block` / `uvm_reg` / `uvm_reg_field`)
extraction with zero C++ dependencies.

## License

Apache-2.0.

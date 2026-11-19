// Bad: connect_phase accidentally calls super.build_phase(phase)
`include "uvm_mock.svh"

class bad_phase_env extends uvm_env;
  `uvm_component_utils(bad_phase_env)
  function new(string name, uvm_component parent);
    super.new(name, parent);
  endfunction

  virtual function void connect_phase(uvm_phase phase);
    super.build_phase(phase);
  endfunction
endclass

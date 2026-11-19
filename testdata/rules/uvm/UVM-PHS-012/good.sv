// Good: connect_phase calls super.connect_phase(phase)
`include "uvm_mock.svh"

class good_phase_env extends uvm_env;
  `uvm_component_utils(good_phase_env)
  function new(string name, uvm_component parent);
    super.new(name, parent);
  endfunction

  virtual function void connect_phase(uvm_phase phase);
    super.connect_phase(phase);
  endfunction
endclass

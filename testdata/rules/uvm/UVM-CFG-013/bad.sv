`include "uvm_mock.svh"
class my_driver extends uvm_driver;
  `uvm_component_utils(my_driver)
  int cfg_delay;
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  task run_phase(uvm_phase phase);
    for (int i = 0; i < 8; i++) begin
      void'(uvm_config_db#(int)::get(this, "", "cfg_delay", cfg_delay));
      #10ns;
    end
  endtask
endclass

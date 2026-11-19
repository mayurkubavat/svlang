// Good: factory override replaces base_pkt with derived child_pkt
`include "uvm_mock.svh"

class base_pkt extends uvm_sequence_item;
  `uvm_object_utils(base_pkt)
  function new(string name = "base_pkt"); super.new(name); endfunction
endclass

class child_pkt extends base_pkt;
  `uvm_object_utils(child_pkt)
  function new(string name = "child_pkt"); super.new(name); endfunction
endclass

class good_override_test extends uvm_test;
  `uvm_component_utils(good_override_test)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  virtual function void build_phase(uvm_phase phase);
    super.build_phase(phase);
    base_pkt::type_id::set_type_override(child_pkt::get_type());
  endfunction
endclass

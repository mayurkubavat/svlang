// Bad: factory override replaces base_pkt with unrelated other_pkt
`include "uvm_mock.svh"

class base_pkt extends uvm_sequence_item;
  `uvm_object_utils(base_pkt)
  function new(string name = "base_pkt"); super.new(name); endfunction
endclass

class other_pkt extends uvm_sequence_item;
  `uvm_object_utils(other_pkt)
  function new(string name = "other_pkt"); super.new(name); endfunction
endclass

class bad_override_test extends uvm_test;
  `uvm_component_utils(bad_override_test)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  virtual function void build_phase(uvm_phase phase);
    super.build_phase(phase);
    base_pkt::type_id::set_type_override(other_pkt::get_type());
  endfunction
endclass

// Good: sequence started on compatible axi_sequencer subclass
`include "uvm_mock.svh"
`ifndef uvm_declare_p_sequencer
`define uvm_declare_p_sequencer(SEQR) SEQR p_sequencer;
`endif

class axi_item extends uvm_sequence_item;
  `uvm_object_utils(axi_item)
  function new(string name = "axi_item"); super.new(name); endfunction
endclass

class axi_sequencer extends uvm_sequencer#(axi_item);
  `uvm_component_utils(axi_sequencer)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
endclass

class axi_ext_sequencer extends axi_sequencer;
  `uvm_component_utils(axi_ext_sequencer)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
endclass

class axi_seq extends uvm_sequence#(axi_item);
  `uvm_object_utils(axi_seq)
  `uvm_declare_p_sequencer(axi_sequencer)
  function new(string name = "axi_seq"); super.new(name); endfunction
endclass

class good_test extends uvm_test;
  `uvm_component_utils(good_test)
  axi_ext_sequencer axi_sqr;
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  virtual task run_phase(uvm_phase phase);
    axi_seq seq;
    seq = axi_seq::type_id::create("seq");
    seq.start(axi_sqr);
  endtask
endclass

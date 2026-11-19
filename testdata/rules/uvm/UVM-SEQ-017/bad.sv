// Bad: sequence with axi_sequencer p_sequencer started on apb_sequencer
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

class apb_sequencer extends uvm_sequencer#(axi_item);
  `uvm_component_utils(apb_sequencer)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
endclass

class axi_seq extends uvm_sequence#(axi_item);
  `uvm_object_utils(axi_seq)
  `uvm_declare_p_sequencer(axi_sequencer)
  function new(string name = "axi_seq"); super.new(name); endfunction
endclass

class bad_test extends uvm_test;
  `uvm_component_utils(bad_test)
  apb_sequencer apb_sqr;
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  virtual task run_phase(uvm_phase phase);
    axi_seq seq;
    seq = axi_seq::type_id::create("seq");
    seq.start(apb_sqr);
  endtask
endclass

// Good: covergroup in uvm_subscriber enables option.per_instance = 1
`include "uvm_mock.svh"

class pkt_item extends uvm_sequence_item;
  `uvm_object_utils(pkt_item)
  bit [1:0] kind;
  function new(string name = "pkt_item"); super.new(name); endfunction
endclass

class good_port_cov extends uvm_subscriber#(pkt_item);
  `uvm_component_utils(good_port_cov)
  pkt_item tr;

  covergroup cg;
    option.per_instance = 1;
    cp_kind: coverpoint tr.kind;
  endgroup

  function new(string name, uvm_component parent);
    super.new(name, parent);
    cg = new();
  endfunction

  virtual function void write(pkt_item t);
    tr = t;
    cg.sample();
  endfunction
endclass

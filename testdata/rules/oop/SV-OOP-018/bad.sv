`include "uvm_mock.svh"
class base_pkt_item extends uvm_sequence_item;
  `uvm_object_utils(base_pkt_item)
  int addr;
  function new(string name = "base_pkt_item"); super.new(name); endfunction
  virtual function void do_copy(uvm_object rhs); endfunction
endclass

class ext_pkt_item extends base_pkt_item;
  `uvm_object_utils(ext_pkt_item)
  int payload;
  function new(string name = "ext_pkt_item"); super.new(name); endfunction
  virtual function void do_copy(uvm_object rhs);
    payload = 0;
  endfunction
endclass

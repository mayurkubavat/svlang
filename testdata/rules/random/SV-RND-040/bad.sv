// Test Case: SV-RND-040 (HardcodedBounds)
`include "uvm_mock.svh"

class base_pkt_item extends uvm_sequence_item;
  `uvm_object_utils(base_pkt_item)
  rand bit [15:0] length;
  rand bit [7:0]  addr;

  // VIOLATION SV-RND-040: hard literal bounds in an extended base class
  constraint c_default_bounds {
    length <= 16'd64;
    addr inside {[8'h10 : 8'h1f]};
  }

  function new(string name = "base_pkt_item");
    super.new(name);
  endfunction : new
endclass : base_pkt_item

class jumbo_pkt_item extends base_pkt_item;
  `uvm_object_utils(jumbo_pkt_item)

  function new(string name = "jumbo_pkt_item");
    super.new(name);
  endfunction : new
endclass : jumbo_pkt_item

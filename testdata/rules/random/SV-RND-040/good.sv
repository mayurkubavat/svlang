// Compliant Test Case: SV-RND-040 (HardcodedBounds)
`include "uvm_mock.svh"

class base_pkt_item #(parameter int unsigned MAX_HW_LEN = 4096) extends uvm_sequence_item;
  `uvm_object_utils(base_pkt_item)
  rand  bit [15:0] length;
  rand  bit [7:0]  addr;
  randc bit [3:0]  tag;

  // Compliant: soft defaults for overridable stimulus, symbolic parameter for HW limit,
  // and randc (where soft is illegal per IEEE 1800 §18.5.14).
  constraint c_default_bounds {
    soft length <= 16'd64;
    length <= MAX_HW_LEN;
    soft addr inside {[8'h10 : 8'h1f]};
    tag < 4'd8;
  }

  function new(string name = "base_pkt_item");
    super.new(name);
  endfunction : new
endclass : base_pkt_item

class jumbo_pkt_item extends base_pkt_item;
  `uvm_object_utils(jumbo_pkt_item)
  constraint c_jumbo { length inside {[16'd512 : 16'd1024]}; }

  function new(string name = "jumbo_pkt_item");
    super.new(name);
  endfunction : new
endclass : jumbo_pkt_item

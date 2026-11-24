// Good: standard IEEE 1800.2 UVM classes only
`include "uvm_mock.svh"

class good_std_item extends uvm_sequence_item;
  `uvm_object_utils(good_std_item)
  function new(string name = "good_std_item");
    super.new(name);
  endfunction
endclass

// Bad: extending an @uvm-contrib UVM class outside IEEE 1800.2
`include "uvm_mock.svh"
`ifndef UVM_MACROS_SVH
class uvm_bit_vector_utils#(type T = bit) extends uvm_object;
endclass
`endif

class my_contrib_bv_helper extends uvm_bit_vector_utils#(bit [7:0]);
endclass

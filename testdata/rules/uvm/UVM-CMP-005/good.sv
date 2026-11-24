`include "uvm_mock.svh"
class my_cb extends uvm_callback;
  `uvm_object_utils(my_cb)
  function new(string name = "my_cb"); super.new(name); endfunction
endclass

class my_driver extends uvm_driver;
  `uvm_component_utils(my_driver)
  `uvm_register_cb(my_driver, my_cb)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
endclass

class my_test extends uvm_test;
  `uvm_component_utils(my_test)
  function new(string name, uvm_component parent); super.new(name, parent); endfunction
  function void connect_phase(uvm_phase phase); uvm_callbacks#(my_driver, my_cb)::add(null, null); endfunction
endclass

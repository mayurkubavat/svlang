// Test Case: UVM-NAM-002 (UvmClassTypeSuffix)
// Extracted from sample_tb.sv (Test Case 163)

`include "uvm_mock.svh"

// --- TEST CASE 163: UVM-NAM-002 (UvmClassTypeSuffix) ---
// VIOLATION UVM-NAM-002: uvm_driver without _driver or _drv suffix!
class BadlyNamedTransmitter extends uvm_driver;
    `uvm_component_utils(BadlyNamedTransmitter)
    function new(string name = "BadlyNamedTransmitter", uvm_component parent = null);
        super.new(name, parent);
    endfunction
endclass

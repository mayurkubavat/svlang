// Test Case: UVM-CMP-009 (ProhibitedDisplayInUvm)
// Extracted from sample_tb.sv (Test Case 106)

`include "uvm_mock.svh"

// --- TEST CASE 106: UVM-CMP-009 (ProhibitedDisplayInUvm) ---
class DisplayInUvmDriver extends uvm_driver;
    `uvm_component_utils(DisplayInUvmDriver)
    function new(string name, uvm_component parent);
        super.new(name, parent);
    endfunction
    task run_phase(uvm_phase phase);
        // VIOLATION UVM-CMP-009: Direct $display bypasses UVM reporting!
        $display("Driver starting transfer...");
    endtask
endclass

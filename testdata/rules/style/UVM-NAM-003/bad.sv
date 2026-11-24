// Test Case: UVM-NAM-003 (ClassMemberPrefix)
// Extracted from sample_tb.sv (Test Case 15)

`include "uvm_mock.svh"

// --- TEST CASE 15: UVM-NAM-003 (Class Member Prefix) ---
class BadPrefixComponent extends uvm_component;
    `uvm_component_utils(BadPrefixComponent)
    // VIOLATION UVM-NAM-003: Member property missing 'm_' prefix!
    int packet_counter;

    function new(string name, uvm_component parent);
        super.new(name, parent);
    endfunction
endclass

// Test Case: UVM-CMP-028 (StaticMemberInUvmComponent)
// Extracted from sample_tb.sv (Test Case 28)

`include "uvm_mock.svh"

// --- TEST CASE 28: UVM-CMP-028 (Non-const Static Member in uvm_component) ---
class MultiChannelMonitor extends uvm_component;
    `uvm_component_utils(MultiChannelMonitor)
    // VIOLATION UVM-CMP-028: Non-const static variable shared across all monitor instances!
    static int m_global_packet_counter = 0;

    function new(string name, uvm_component parent);
        super.new(name, parent);
    endfunction
endclass

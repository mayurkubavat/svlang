// Test Case: SVL-OOP-010 (ChainedAssignmentAliasing)
// Extracted from sample_tb.sv (Test Case 187)

`include "uvm_mock.svh"

// --- TEST CASE 187: SVL-OOP-010 (ChainedAssignmentAliasing) ---
class ChainedAssignBench;
    BasePacket p1, p2, p3;
    function void alias_handles();
        p3 = new();
        // VIOLATION SVL-OOP-010: Chained handle assignment creates subtle object aliasing!
        p1 = p2 = p3;
    endfunction
endclass

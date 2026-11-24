// Test Case: SVL-RND-009 (CyclicSolveBefore)
// Extracted from sample_tb.sv (Test Case 104)

`include "uvm_mock.svh"

// --- TEST CASE 104: SVL-RND-009 (CyclicSolveBefore) ---
class CyclicSolveBench;
    rand int a;
    rand int b;
    constraint c_cycle {
        // VIOLATION SVL-RND-009: Cyclic dependency in solve..before!
        solve a before b;
        solve b before a;
    }
endclass

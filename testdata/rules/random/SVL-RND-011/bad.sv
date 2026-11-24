// Test Case: SVL-RND-011 (ZeroWeightDistribution)
// Extracted from sample_tb.sv (Test Case 119)

`include "uvm_mock.svh"

// --- TEST CASE 119: SVL-RND-011 (ZeroWeightDistribution) ---
class ZeroWeightDistBench;
    rand int unsigned opcode;
    constraint c_bad_dist {
        // VIOLATION SVL-RND-011: All distribution weights evaluate to zero!
        opcode dist { 0 := 0, 1 := 0, 2 := 0 };
    }
endclass

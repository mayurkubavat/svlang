// Test Case: SVL-RND-016 (RandcStateExhaustion)
// Extracted from sample_tb.sv (Test Case 176)

`include "uvm_mock.svh"

// --- TEST CASE 176: SVL-RND-016 (RandcStateExhaustion) ---
class NarrowRandcBench;
    // VIOLATION SVL-RND-016: randc declared on 1-bit variable (only 2 states)!
    randc bit flag_mode;
    // VIOLATION SVL-RND-016: randc declared on 2-bit variable (only 4 states)!
    randc bit [1:0] state_sel;
endclass

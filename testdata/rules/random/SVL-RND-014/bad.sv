// Test Case: SVL-RND-014 (UnconstrainedRandVariable)
// Extracted from sample_tb.sv (Test Case 154)

`include "uvm_mock.svh"

// --- TEST CASE 154: SVL-RND-014 (UnconstrainedRandVariable) ---
class UnconstrainedRandBench;
    rand int constrained_field;
    // VIOLATION SVL-RND-014: rand variable is never referenced in any constraint!
    rand bit [7:0] unconstrained_mode;
    constraint c_field { constrained_field inside {[0:100]}; }
endclass

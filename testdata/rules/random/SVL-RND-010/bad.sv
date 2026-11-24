// Test Case: SVL-RND-010 (RedundantConstraint)
// Extracted from sample_tb.sv (Test Case 105)

`include "uvm_mock.svh"

// --- TEST CASE 105: SVL-RND-010 (RedundantConstraint) ---
class DuplicateConstraintBench;
    rand int burst_len;
    constraint c_limits {
        burst_len > 0;
        burst_len < 256;
        // VIOLATION SVL-RND-010: Duplicate constraint expression!
        burst_len > 0;
    }
endclass

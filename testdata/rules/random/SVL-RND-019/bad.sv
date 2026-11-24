// Test Case: SVL-RND-019 (RandcInDynamicContainer)
// Extracted from sample_tb.sv (Test Case 203)

`include "uvm_mock.svh"

// --- TEST CASE 203: SVL-RND-019 (RandcInDynamicContainer) ---
class RandcDynContainerBench;
    // VIOLATION SVL-RND-019: randc on dynamic array container!
    randc bit [7:0] dyn_payload[];
    // VIOLATION SVL-RND-019: randc on queue container!
    randc bit [7:0] q_payload[$];
endclass : RandcDynContainerBench

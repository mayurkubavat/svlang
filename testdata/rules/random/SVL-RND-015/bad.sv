// Test Case: SVL-RND-015 (ConcurrentRandomizeInFork)
// Extracted from sample_tb.sv (Test Case 162)

`include "uvm_mock.svh"

// --- TEST CASE 162: SVL-RND-015 (ConcurrentRandomizeInFork) ---
class ConcurrentRandBench;
    ShadowItem shared_item;
    task test_concurrent_rand();
        shared_item = new();
        fork
            // VIOLATION SVL-RND-015: Parallel threads randomizing the same object handle!
            shared_item.randomize();
            shared_item.randomize();
        join
    endtask
endclass

// Test Case: SVL-CON-006 (DisableForkWithoutScope)
// Extracted from sample_tb.sv (Test Case 192)

`include "uvm_mock.svh"

// --- TEST CASE 192: SVL-CON-006 (DisableForkWithoutScope) ---
class DisableForkScopeBench;
    task run_threads();
        #10;
        // VIOLATION SVL-CON-006: disable fork at routine scope without isolating fork..join block!
        disable fork;
    endtask : run_threads
endclass : DisableForkScopeBench

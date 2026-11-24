// Test Case: SVL-CON-001 (ConstantWaitCondition)
// Extracted from sample_tb.sv (Test Case 147)

`include "uvm_mock.svh"

// --- TEST CASE 147: SVL-CON-001 (ConstantWaitCondition) ---
class ConstantWaitBench;
    task wait_deadlock();
        // VIOLATION SVL-CON-001: wait(0) deadlocks simulation thread!
        wait(0);
    endtask
endclass

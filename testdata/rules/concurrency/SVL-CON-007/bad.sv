// Test Case: SVL-CON-007 (WaitModifiedInLoop)
// Extracted from sample_tb.sv (Test Case 194)

`include "uvm_mock.svh"

// --- TEST CASE 194: SVL-CON-007 (WaitModifiedInLoop) ---
class WaitModifiedBench;
    bit loop_flag;
    task monitor_loop();
        while (1) begin
            // VIOLATION SVL-CON-007: Condition variable 'loop_flag' in wait() is never modified inside loop!
            wait(loop_flag);
            #10;
        end
    endtask : monitor_loop
endclass : WaitModifiedBench

// Test Case: SVL-CON-008 (ForkLoopVariableCapture)
// Extracted from sample_tb.sv (Test Case 200)

`include "uvm_mock.svh"

// --- TEST CASE 200: SVL-CON-008 (ForkLoopVariableCapture) ---
class ForkLoopCaptureBench;
    task spawn_workers();
        for (int i = 0; i < 4; i++) begin
            fork
                // VIOLATION SVL-CON-008: Loop index 'i' captured directly by reference in fork..join_none!
                #10 $display("Worker ID: %0d", i);
            join_none
        end
    endtask : spawn_workers
endclass : ForkLoopCaptureBench

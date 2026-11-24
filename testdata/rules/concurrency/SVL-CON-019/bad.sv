// Test Case: SVL-CON-019 (RepeatLoopWithoutTiming)
// Extracted from sample_tb.sv (Test Case 143)

`include "uvm_mock.svh"

// --- TEST CASE 143: SVL-CON-019 (RepeatLoopWithoutTiming) ---
class RepeatTimingBench;
    task poll_status();
        int attempts;
        // VIOLATION SVL-CON-019: Repeat loop in task contains no time-consuming statements!
        repeat (100) begin
            attempts++;
        end
    endtask
endclass

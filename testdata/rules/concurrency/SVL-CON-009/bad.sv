// Test Case: SVL-CON-009 (BlockingEventTriggerInClockedBlock)
// Extracted from sample_tb.sv (Test Case 213)

`include "uvm_mock.svh"

// --- TEST CASE 213: SVL-CON-009 (BlockingEventTriggerInClockedBlock) ---
module BlockingEventClockedBench(input logic clk);
    event sync_ev;
    always @(posedge clk) begin
        // VIOLATION SVL-CON-009: Blocking event trigger '-> sync_ev' in sequential block!
        -> sync_ev;
    end
endmodule : BlockingEventClockedBench

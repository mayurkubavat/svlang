// Test Case: SVA-PRP-009 (TimeInAssertionAction)
// Extracted from sample_tb.sv (Test Case 113)

`include "uvm_mock.svh"

// --- TEST CASE 113: SVA-PRP-009 (TimeInAssertionAction) ---
module TimeInActionBench;
    logic clk, req, gnt;
    always @(posedge clk) begin
        // VIOLATION SVA-PRP-009: Time-consuming delay in assertion else block!
        assert (req == gnt) else begin
            #5;
            $error("Req/Gnt mismatch");
        end
    end
endmodule

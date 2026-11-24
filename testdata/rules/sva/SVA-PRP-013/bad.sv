// Test Case: SVA-PRP-013 (UnlabeledAssertion)
// Extracted from sample_tb.sv (Test Case 97)

`include "uvm_mock.svh"

// --- TEST CASE 97: SVA-PRP-013 (UnlabeledAssertion) ---
module UnlabeledAssertBench(input logic clk, input logic valid);
    // VIOLATION SVA-PRP-013: Immediate assertion without a label!
    always_comb begin
        assert (valid);
    end
endmodule

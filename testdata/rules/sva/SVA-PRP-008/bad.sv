// Test Case: SVA-PRP-008 (AssertWithoutElse)
// Extracted from sample_tb.sv (Test Case 99)

`include "uvm_mock.svh"

// --- TEST CASE 99: SVA-PRP-008 (AssertWithoutElse) ---
module AssertNoElseBench(input logic clk, input logic req);
    always_comb begin
        check_req: assert (req); // VIOLATION SVA-PRP-008: Missing else action block!
    end
endmodule

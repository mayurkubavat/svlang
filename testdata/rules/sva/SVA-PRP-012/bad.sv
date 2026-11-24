// Test Case: SVA-PRP-012 (SvaUnboundedDelay)
// Extracted from sample_tb.sv (Test Case 73)

`include "uvm_mock.svh"

// --- TEST CASE 73: SVA-PRP-012 (Unbounded Sequence Delay Cycle Range) ---
module sva_unbounded_delay_tester(input logic clk, input logic req, input logic ack);
    // VIOLATION SVA-PRP-012: Unbounded cycle delay ##[1:$] causes simulation thread leaks and FPV timeouts!
    property p_unbounded_ack;
        @(posedge clk) req |-> ##[1:$] ack;
    endproperty
    assert property (p_unbounded_ack);
endmodule

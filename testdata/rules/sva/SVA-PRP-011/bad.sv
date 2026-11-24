// Test Case: SVA-PRP-011 (ConcurrentAssertInFunction)
// Extracted from sample_tb.sv (Test Case 98)

`include "uvm_mock.svh"

// --- TEST CASE 98: SVA-PRP-011 (ConcurrentAssertInFunction) ---
class AssertInFuncBench;
    function void evaluate_signals(logic clk, logic req, logic gnt);
        // VIOLATION SVA-PRP-011: Concurrent assertion inside function!
        assert property (@(posedge clk) req |-> ##1 gnt);
    endfunction
endclass

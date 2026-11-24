// Test Case: SVL-COR-002 (NonShortCircuitLogical)
// Extracted from sample_tb.sv (Test Case 149)

`include "uvm_mock.svh"

// --- TEST CASE 149: SVL-COR-002 (NonShortCircuitLogical) ---
class NonShortCircuitBench;
    function bit check_valid(MockTransaction tr);
        // VIOLATION SVL-COR-002: Bitwise '&' does not short-circuit, risking null pointer dereference!
        if (tr != null & tr.addr > 0) begin
            return 1;
        end
        return 0;
    endfunction
endclass

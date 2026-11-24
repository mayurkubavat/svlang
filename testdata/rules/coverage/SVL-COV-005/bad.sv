// Test Case: SVL-COV-005 (UnboundedTransitionBins)
// Extracted from sample_tb.sv (Test Case 114)

`include "uvm_mock.svh"

// --- TEST CASE 114: SVL-COV-005 (UnboundedTransitionBins) ---
class UnboundedTransBench;
    logic [1:0] state;
    covergroup trans_cg;
        cp_state: coverpoint state {
            // VIOLATION SVL-COV-005: Unbounded transition repetition [*1:$]!
            bins b_infinite = (0 => 1[*1:$] => 2);
        }
    endgroup
    function new();
        trans_cg = new();
    endfunction
endclass

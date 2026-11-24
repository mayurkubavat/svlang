// Test Case: RTL-DAT-001 (CaseWithoutDefault)
// Extracted from sample_tb.sv (Test Case 155)

`include "uvm_mock.svh"

// --- TEST CASE 155: RTL-DAT-001 (CaseWithoutDefault) ---
class CaseDefaultBench;
    function void decode_state(int state);
        // VIOLATION RTL-DAT-001: case statement lacks default fallback branch!
        case (state)
            0: $display("Idle");
            1: $display("Active");
        endcase
    endfunction
endclass

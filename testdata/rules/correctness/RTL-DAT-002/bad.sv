// Test Case: RTL-DAT-002 (CasexProhibited)
// Extracted from sample_tb.sv (Test Case 181)

`include "uvm_mock.svh"

// --- TEST CASE 181: RTL-DAT-002 (CasexProhibited) ---
module casex_violation_mod;
    logic [3:0] sel;
    logic [1:0] out;
    always_comb begin
        // VIOLATION RTL-DAT-002: casex treats unknown 'x' as wildcards, masking simulation bugs!
        casex (sel)
            4'b1xx0: out = 2'b00;
            4'b01xx: out = 2'b01;
            default: out = 2'b10;
        endcase
    end
endmodule

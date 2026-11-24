// Test Case: SVL-STY-009 (OneStatementPerLine)
// Extracted from sample_tb.sv (Test Case 46)

`include "uvm_mock.svh"

// --- TEST CASE 46: SVL-STY-009 (Multiple Procedural Statements on Same Line) ---
module single_stmt_per_line_tester;
    logic a, b, c;

    initial begin
        // VIOLATION SVL-STY-009: Multiple statements on the same line separated by semicolon!
        a = 1; b = 0; c = 1;
    end
endmodule

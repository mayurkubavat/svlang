// Test Case: SVL-COR-006 (ProceduralAssignmentAsSubExpression)
// Extracted from sample_tb.sv (Test Case 220)

`include "uvm_mock.svh"

// --- TEST CASE 220: SVL-COR-006 (ProceduralAssignmentAsSubExpression) ---
class EmbeddedAssignBench;
    int a, b, c;
    function void test_expr();
        // VIOLATION SVL-COR-006: Assignment expression used as sub-expression!
        if ((a = b) > 0) begin
            c = (a = 1) + 2;
        end
    endfunction : test_expr
endclass : EmbeddedAssignBench

// Test Case: SVL-COR-004 (SelfComparison)
// Extracted from sample_tb.sv (Test Case 172)

`include "uvm_mock.svh"

// --- TEST CASE 172: SVL-COR-004 (SelfComparison) ---
class SelfCompBench;
    function void test_comp(int pkt_id);
        // VIOLATION SVL-COR-004: Variable compared to itself!
        if (pkt_id == pkt_id) begin
            $display("Self comparison tautology");
        end
    endfunction
endclass

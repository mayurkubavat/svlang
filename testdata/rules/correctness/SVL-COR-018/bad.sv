// Test Case: SVL-COR-018 (IntraAssignmentDelay)
// Extracted from sample_tb.sv (Test Case 131)

`include "uvm_mock.svh"

// --- TEST CASE 131: SVL-COR-018 (IntraAssignmentDelay) ---
class IntraDelayBench;
    int a, b;
    task run_pipeline();
        // VIOLATION SVL-COR-018: Intra-assignment delay on blocking assignment!
        a = #5 b;
    endtask
endclass

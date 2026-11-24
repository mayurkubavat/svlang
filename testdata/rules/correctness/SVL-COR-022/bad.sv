// Test Case: SVL-COR-022 (NonBlockingInFunction)
// Extracted from sample_tb.sv (Test Case 111)

`include "uvm_mock.svh"

// --- TEST CASE 111: SVL-COR-022 (NonBlockingInFunction) ---
class NonBlockingFuncBench;
    logic [7:0] crc;
    function void update_crc(logic [7:0] byte_in);
        // VIOLATION SVL-COR-022: Non-blocking assignment in function!
        crc <= byte_in ^ 8'h55;
    endfunction
endclass

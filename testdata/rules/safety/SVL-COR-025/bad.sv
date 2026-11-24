// Test Case: SVL-COR-025 (StreamingConcatenationSizeMismatch)
// Extracted from sample_tb.sv (Test Case 133)

`include "uvm_mock.svh"

// --- TEST CASE 133: SVL-COR-025 (StreamingConcatenationSizeMismatch) ---
class StreamBench;
    byte unsigned byte_stream[$];
    int header, payload;
    function void pack_data();
        // VIOLATION SVL-COR-025: Streaming operator omits explicit slice width!
        byte_stream = {>> {header, payload}};
    endfunction
endclass

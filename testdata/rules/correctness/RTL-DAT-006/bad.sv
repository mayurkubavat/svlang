// Test Case: RTL-DAT-006 (PartSelectReversed)
// Extracted from sample_tb.sv (Test Case 100)

`include "uvm_mock.svh"

// --- TEST CASE 100: RTL-DAT-006 (PartSelectReversed) ---
class PartSelectBench;
    logic [7:0] bus_data;
    function void extract_nibble();
        logic [3:0] nibble;
        // VIOLATION RTL-DAT-006: Inverted part-select [0:3] on descending [7:0] vector!
        nibble = bus_data[0:3];
    endfunction
endclass

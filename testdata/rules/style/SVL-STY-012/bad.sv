// Test Case: SVL-STY-012 (TrailingWhitespace)
// Extracted from sample_tb.sv (Test Case 118)

`include "uvm_mock.svh"

// --- TEST CASE 118: SVL-STY-012 (TrailingWhitespace) ---
class TrailingWhitespaceBench;
    // VIOLATION SVL-STY-012: Trailing whitespace at end of line!    
    int dummy_val = 42;
endclass

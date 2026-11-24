// Test Case: SVL-PRT-002 (HardcodedHostPaths)
// Extracted from sample_tb.sv (Test Case 71)

`include "uvm_mock.svh"

// --- TEST CASE 71: SVL-PRT-002 (Hardcoded Absolute Host Filesystem Path) ---
module hardcoded_path_tester;
    logic [31:0] mem [0:255];
    initial begin
        // VIOLATION SVL-PRT-002: Absolute path /home/user/test.hex breaks regression portability!
        $readmemh("/home/user/test.hex", mem);
    end
endmodule

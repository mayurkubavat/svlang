// Test Case: SVL-PRT-001 (ProprietaryVendorCalls)
// Extracted from sample_tb.sv (Test Case 72)

`include "uvm_mock.svh"

// --- TEST CASE 72: SVL-PRT-001 (Unguarded Proprietary Vendor Simulator Tasks) ---
module proprietary_vendor_tester;
    initial begin
        // VIOLATION SVL-PRT-001: Unguarded $fsdbDumpvars call breaks multi-vendor toolchain portability!
        $fsdbDumpvars(0, "top");
    end
endmodule

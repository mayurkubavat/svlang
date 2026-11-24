// Test Case: SVL-DEP-007 (LegacyRegWireInTestbench)
// Extracted from sample_tb.sv (Test Case 60)

`include "uvm_mock.svh"

// --- TEST CASE 60: SVL-DEP-007 (Legacy Reg in SystemVerilog) ---
module legacy_reg_wire_tester;
    // VIOLATION SVL-DEP-007: Legacy 'reg' in SystemVerilog!
    reg [7:0] old_reg;
endmodule

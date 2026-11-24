// Test Case: SV-BAN-001 (LegacyRegWireInTestbench)
// Extracted from sample_tb.sv (Test Case 60)

`include "uvm_mock.svh"

// --- TEST CASE 60: SV-BAN-001 (Legacy Reg in SystemVerilog) ---
module legacy_reg_wire_tester;
    // VIOLATION SV-BAN-001: Legacy 'reg' in SystemVerilog!
    reg [7:0] old_reg;
endmodule

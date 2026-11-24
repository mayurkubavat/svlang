// Test Case: SVL-COR-026 (TimescalePrecisionMismatch)
// Extracted from sample_tb.sv (Test Case 148)

`include "uvm_mock.svh"

// --- TEST CASE 148: SVL-COR-026 (TimescalePrecisionMismatch) ---
// VIOLATION SVL-COR-026: Time precision 10ns is coarser than time unit 1ns!
`timescale 1ns / 10ns
module MockTimescaleModule;
endmodule

// Test Case: SVL-DPI-002 (DpiTypeWidthAlignment)
// Extracted from sample_tb.sv (Test Case 78)

`include "uvm_mock.svh"

// --- TEST CASE 78: SVL-DPI-002 (4-State Types Across DPI-C Boundary) ---
// VIOLATION SVL-DPI-002: 4-state 'integer' requires complex svLogicVecVal struct in C instead of clean C99 int32_t!
import "DPI-C" function void dpi_calculate_crc(input integer seed_val);

// Test Case: SVL-PRT-003 (GlobalUnitPollution)
// Extracted from sample_tb.sv (Test Case 79)

`include "uvm_mock.svh"

// --- TEST CASE 79: SVL-PRT-003 ($unit Global Scope Pollution) ---
// VIOLATION SVL-PRT-003: Global variable leaked into $unit compilation scope outside any package or module!
int leaked_global_packet_id;

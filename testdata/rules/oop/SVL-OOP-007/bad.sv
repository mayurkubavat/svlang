// Test Case: SVL-OOP-007 (NonVirtualInterfaceInClass)
// Extracted from sample_tb.sv (Test Case 157)

`include "uvm_mock.svh"

// --- TEST CASE 157: SVL-OOP-007 (NonVirtualInterfaceInClass) ---
class NonVirtualIfBench;
    // VIOLATION SVL-OOP-007: Interface handle in class declared without 'virtual' keyword!
    mock_bus_if bus_if;
endclass

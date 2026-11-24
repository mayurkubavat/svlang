// Test Case: SVL-NAM-002 (InterfaceNamingConvention)
// Extracted from sample_tb.sv (Test Case 124)

`include "uvm_mock.svh"

// --- TEST CASE 124: SVL-NAM-002 (InterfaceNamingConvention) ---
// VIOLATION SVL-NAM-002: Interface lacking _if suffix!
interface custom_uart_bus;
    logic rx;
    logic tx;
endinterface

// Test Case: SVL-NAM-001 (PackageNamingConvention)
// Extracted from sample_tb.sv (Test Case 123)

`include "uvm_mock.svh"

// --- TEST CASE 123: SVL-NAM-001 (PackageNamingConvention) ---
// VIOLATION SVL-NAM-001: Package lacking _pkg suffix!
package telemetry_defs;
    typedef int counter_t;
endpackage

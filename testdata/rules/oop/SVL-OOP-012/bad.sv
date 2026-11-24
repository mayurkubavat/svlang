// Test Case: SVL-OOP-012 (SuperNewNotFirstStatement)
// Extracted from sample_tb.sv (Test Case 208)

`include "uvm_mock.svh"

// --- TEST CASE 208: SVL-OOP-012 (SuperNewNotFirstStatement) ---
class BaseCtorBench;
    int base_val;
    function new(int v = 0);
        base_val = v;
    endfunction : new
endclass : BaseCtorBench

class DerivedLateSuperNewBench extends BaseCtorBench;
    int derived_val;
    function new();
        // VIOLATION SVL-OOP-012: Procedural statement executed before super.new()!
        derived_val = 42;
        super.new(10);
    endfunction : new
endclass : DerivedLateSuperNewBench

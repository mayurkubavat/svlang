// Test Case: SVL-RND-017 (RandomizeInConstructor)
// Extracted from sample_tb.sv (Test Case 185)

`include "uvm_mock.svh"

// --- TEST CASE 185: SVL-RND-017 (RandomizeInConstructor) ---
class RandomizeCtorBench;
    rand int data_val;
    function new();
        // VIOLATION SVL-RND-017: Calling randomize() inside class constructor breaks inheritance!
        void'(this.randomize());
    endfunction
endclass

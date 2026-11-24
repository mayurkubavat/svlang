// Test Case: SVL-RND-008 (StaticConstraint)
// Extracted from sample_tb.sv (Test Case 103)

`include "uvm_mock.svh"

// --- TEST CASE 103: SVL-RND-008 (StaticConstraint) ---
class StaticConstraintItem extends uvm_sequence_item;
    `uvm_object_utils(StaticConstraintItem)
    rand int len;
    // VIOLATION SVL-RND-008: Static constraint leaks across all instances!
    static constraint c_global_len {
        len inside {[1:100]};
    }
    function new(string name = "StaticConstraintItem");
        super.new(name);
    endfunction
endclass

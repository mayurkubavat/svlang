// Test Case: SVL-MNT-053 (MagicDelayInSequence)
// Extracted from sample_tb.sv (Test Case 89)

`include "uvm_mock.svh"

// --- TEST CASE 89: SVL-MNT-053 (MagicDelayInSequence) ---
class MagicDelaySequence extends uvm_sequence;
    `uvm_object_utils(MagicDelaySequence)
    function new(string name = "MagicDelaySequence");
        super.new(name);
    endfunction
    task body();
        // VIOLATION SVL-MNT-053: Hardcoded physical delay inside sequence!
        #100;
    endtask
endclass

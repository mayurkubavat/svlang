// Test Case: SVL-DED-004 (ConsecutiveDeadStore)
// Extracted from sample_tb.sv (Test Case 35)

`include "uvm_mock.svh"

// --- TEST CASE 35: SVL-DED-004 (Consecutive Dead Store) ---
class DeadStoreTester extends uvm_object;
    `uvm_object_utils(DeadStoreTester)
    function new(string name = "DeadStoreTester");
        super.new(name);
    endfunction

    function int test_dead_store(int raw_in);
        int calc;
        // VIOLATION SVL-DED-004: calc is assigned 0 and immediately overwritten by next statement without being read!
        calc = 0;
        calc = raw_in * 4;
        return calc;
    endfunction
endclass

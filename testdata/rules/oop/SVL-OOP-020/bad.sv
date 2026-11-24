// Test Case: SVL-OOP-020 (ForeachLoopVariableShadowing)
// Extracted from sample_tb.sv (Test Case 151)

`include "uvm_mock.svh"

// --- TEST CASE 151: SVL-OOP-020 (ForeachLoopVariableShadowing) ---
class ForeachShadowBench;
    int idx;
    int data_queue[$];
    function void process_items();
        // VIOLATION SVL-OOP-020: Implicit foreach index 'idx' shadows class member 'idx'!
        foreach (data_queue[idx]) begin
            data_queue[idx] = idx * 2;
        end
    endfunction
endclass

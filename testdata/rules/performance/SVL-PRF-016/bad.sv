// Test Case: SVL-PRF-016 (ClassInstantiationInLoop)
// Extracted from sample_tb.sv (Test Case 174)

`include "uvm_mock.svh"

// --- TEST CASE 174: SVL-PRF-016 (ClassInstantiationInLoop) ---
class NewInLoopBench;
    task generate_items();
        ShadowItem tr;
        for (int i = 0; i < 100; i++) begin
            // VIOLATION SVL-PRF-016: Instantiating objects via new() inside a loop!
            tr = new();
        end
    endtask
endclass

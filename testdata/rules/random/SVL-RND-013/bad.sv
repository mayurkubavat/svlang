// Test Case: SVL-RND-013 (EmptyRandomizeWith)
// Extracted from sample_tb.sv (Test Case 146)

`include "uvm_mock.svh"

// --- TEST CASE 146: SVL-RND-013 (EmptyRandomizeWith) ---
class EmptyRandomizeWithBench;
    task run_test();
        MockTransaction tr = new();
        // VIOLATION SVL-RND-013: randomize() with empty inline constraint block!
        if (!tr.randomize() with {}) begin
            $display("Randomization failed");
        end
    endtask
endclass

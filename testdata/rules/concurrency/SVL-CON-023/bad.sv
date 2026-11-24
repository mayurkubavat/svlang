// Test Case: SVL-CON-023 (ForkInsideFunction)
// Extracted from sample_tb.sv (Test Case 130)

`include "uvm_mock.svh"

// --- TEST CASE 130: SVL-CON-023 (ForkInsideFunction) ---
class ForkInsideFunctionBench;
    function void launch_background_poll();
        // VIOLATION SVL-CON-023: Spawning background thread via fork inside function!
        fork
            $display("Polling background...");
        join_none
    endfunction
endclass

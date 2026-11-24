// Test Case: SVL-CON-005 (WaitForkWithoutFork)
// Extracted from sample_tb.sv (Test Case 184)

`include "uvm_mock.svh"

// --- TEST CASE 184: SVL-CON-005 (WaitForkWithoutFork) ---
class WaitForkNoSpawnBench;
    task wait_all_children();
        #10;
        // VIOLATION SVL-CON-005: Routine contains wait fork but spawned no parallel threads!
        wait fork;
    endtask
endclass

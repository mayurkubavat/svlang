// Test Case: SVL-PRF-015 (ArrayLocatorEmptyQueueDeref)
// Extracted from sample_tb.sv (Test Case 128)

`include "uvm_mock.svh"

// --- TEST CASE 128: SVL-PRF-015 (ArrayLocatorEmptyQueueDeref) ---
class ArrayLocatorBench;
    int items[$];
    function int get_first_high();
        // VIOLATION SVL-PRF-015: Immediate dereferencing [0] on locator queue without size check!
        return items.find(x) with (x > 100)[0];
    endfunction
endclass

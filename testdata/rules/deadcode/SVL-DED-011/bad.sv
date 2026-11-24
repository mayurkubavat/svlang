// Test Case: SVL-DED-011 (EmptyTaskOrFunction)
// Extracted from sample_tb.sv (Test Case 182)

`include "uvm_mock.svh"

// --- TEST CASE 182: SVL-DED-011 (EmptyTaskOrFunction) ---
class EmptyRoutineBench;
    // VIOLATION SVL-DED-011: Concrete routine with zero executable statements!
    function void process_handshake();
    endfunction
endclass

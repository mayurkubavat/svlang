// Test Case: SVL-CON-026 (ZeroDelayEventTrigger)
// Extracted from sample_tb.sv (Test Case 132)

`include "uvm_mock.svh"

// --- TEST CASE 132: SVL-CON-026 (ZeroDelayEventTrigger) ---
class EventTriggerBench;
    event sync_event;
    task signal_ready();
        // VIOLATION SVL-CON-026: Blocking event trigger causes zero-delay race!
        -> sync_event;
    endtask
endclass

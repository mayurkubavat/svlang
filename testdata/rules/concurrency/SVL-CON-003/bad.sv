// Test Case: SVL-CON-003 (UnboundedMailboxInstantiation)
// Extracted from sample_tb.sv (Test Case 169)

`include "uvm_mock.svh"

// --- TEST CASE 169: SVL-CON-003 (UnboundedMailboxInstantiation) ---
class UnboundedMailboxBench;
    mailbox #(int) data_mb;
    function new();
        // VIOLATION SVL-CON-003: Mailbox instantiated without bound limit (unbounded)!
        data_mb = new();
    endfunction
endclass

// Test Case: SVL-CON-025 (TryPutIgnoredReturn)
// Extracted from sample_tb.sv (Test Case 68)

`include "uvm_mock.svh"

// --- TEST CASE 68: SVL-CON-025 (Discarded Return on Non-Blocking IPC) ---
class DiscardedIpcTester;
    uvm_tlm_fifo #(int) fifo;
    function void send_item(int item);
        // VIOLATION SVL-CON-025: Discarding return value of try_put drops packet when FIFO is full!
        void'(fifo.try_put(item));
    endfunction
endclass

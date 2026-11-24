// Test Case: RTL-SYN-007 (EventTriggerInAlwaysComb)
// Extracted from sample_tb.sv (Test Case 164)

`include "uvm_mock.svh"

// --- TEST CASE 164: RTL-SYN-007 (EventTriggerInAlwaysComb) ---
module comb_event_trigger_mod;
    event comb_ev;
    int comb_data;
    always_comb begin
        comb_data = 1;
        // VIOLATION RTL-SYN-007: Event trigger inside always_comb!
        -> comb_ev;
    end
endmodule

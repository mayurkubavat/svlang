// Test Case: SVL-OOP-008 (EmptyVirtualMethodWithoutPure)
// Extracted from sample_tb.sv (Test Case 166)

`include "uvm_mock.svh"

// --- TEST CASE 166: SVL-OOP-008 (EmptyVirtualMethodWithoutPure) ---
virtual class AbstractDriverBase;
    // VIOLATION SVL-OOP-008: Empty virtual method body without pure virtual!
    virtual function void send_packet();
    endfunction
endclass

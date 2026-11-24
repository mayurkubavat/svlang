// Test Case: SVL-RND-004 (InlineShadowNoLocal)
// Extracted from sample_tb.sv (Test Case 65)

`include "uvm_mock.svh"

// --- TEST CASE 65: SVL-RND-004 (Inline Constraint Variable Shadowing) ---
class ShadowItem;
    rand bit [31:0] addr;
endclass

class InlineShadowTester;
    task drive_address(bit [31:0] addr);
        ShadowItem item = new();
        // VIOLATION SVL-RND-004: addr == addr shadows local addr without local:: qualifier!
        item.randomize() with { addr == addr; };
    endtask
endclass

// Test Case: UVM-DEP-007 (UvmBannedUvmDoMacros)
// Extracted from sample_tb.sv (Test Case 59)

`include "uvm_mock.svh"

// --- TEST CASE 59: UVM-DEP-007 (UVM Banned `uvm_do Macros) ---
class BannedUvmDoSeq extends uvm_sequence;
    `uvm_object_utils(BannedUvmDoSeq)
    function new(string name = "BannedUvmDoSeq");
        super.new(name);
    endfunction

    task body();
        BannedFieldMacroItem item;
        // VIOLATION UVM-DEP-007: `uvm_do macro obscures driver handshake!
        `uvm_do(item)
    endtask
endclass

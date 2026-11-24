// Test Case: UVM-DEP-006 (UvmBannedFieldMacros)
// Extracted from sample_tb.sv (Test Case 58)

`include "uvm_mock.svh"

// --- TEST CASE 58: UVM-DEP-006 (UVM Banned Field Macros) ---
class BannedFieldMacroItem extends uvm_sequence_item;
    int addr;
    // VIOLATION UVM-DEP-006: `uvm_field_* macro suite is banned in high-performance DV!
    `uvm_object_utils_begin(BannedFieldMacroItem)
        `uvm_field_int(addr, 0)
    `uvm_object_utils_end

    function new(string name = "BannedFieldMacroItem");
        super.new(name);
    endfunction
endclass

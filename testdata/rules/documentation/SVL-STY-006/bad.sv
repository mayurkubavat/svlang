// Test Case: SVL-STY-006 (SectionHeaderBanner)
// Extracted from sample_tb.sv (Test Case 43)

`include "uvm_mock.svh"

// --- TEST CASE 43: SVL-STY-005 & SVL-STY-006 (Ragged Separator and Section Banners) ---
// VIOLATION SVL-STY-006: Ragged section banner width (20 characters instead of 70-80)
//==================
// Section: Helpers
//==================

// VIOLATION SVL-STY-005: Ragged method separator banner (16 dashes instead of 70-80)
//----------------
function void ragged_banner_demo();
    $display("Demo banner");
endfunction : ragged_banner_demo

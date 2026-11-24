// Test Case: SVL-STY-001 (MacroTrailingSemicolon)
// Extracted from sample_tb.sv (Test Case 5)

`include "uvm_mock.svh"

// --- TEST CASE 5: SVL-STY-001 (Trailing semicolon after macro call) ---
class StyleViolationComponent extends uvm_component;
    function new(string name, uvm_component parent);
        super.new(name, parent);
        // VIOLATION SVL-STY-001: whitespace may separate a macro's name from its arguments
        `uvm_info ("STY", "spaced", UVM_LOW);
    endfunction

    // VIOLATION SVL-STY-001: Trailing semicolon after macro invocation in declarative scope
    `uvm_component_utils(StyleViolationComponent);
endclass


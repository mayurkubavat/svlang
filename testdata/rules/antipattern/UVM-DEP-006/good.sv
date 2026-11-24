// Compliant Clean Test Case: UVM-DEP-006 (UvmBannedFieldMacros)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_uvmbannedfieldmacros extends uvm_object;
    function new(string name = "compliant");
        super.new(name); // `uvm_field_enum in a comment must not fire
    endfunction
endclass

// Compliant Clean Test Case: SVL-NAM-003 (TypedefNameSuffix)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_typedefnamesuffix extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

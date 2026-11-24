// Compliant Clean Test Case: SVL-MNT-021 (DuplicateMethodBody)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_duplicatemethodbody extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

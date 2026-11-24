// Compliant Clean Test Case: SVL-OOP-010 (ChainedAssignmentAliasing)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_chainedassignmentaliasing extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

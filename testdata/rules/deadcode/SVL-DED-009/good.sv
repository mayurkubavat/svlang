// Compliant Clean Test Case: SVL-DED-009 (EmptyAlwaysOrInitial)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_emptyalwaysorinitial extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

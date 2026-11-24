// Compliant Clean Test Case: SVL-CON-023 (ForkInsideFunction)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_forkinsidefunction extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

// Compliant Clean Test Case: SVL-OOP-012 (SuperNewNotFirstStatement)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_supernewnotfirststatement extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

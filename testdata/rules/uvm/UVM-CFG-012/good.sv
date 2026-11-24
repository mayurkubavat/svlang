// Compliant Clean Test Case: UVM-CFG-012 (UvmDanglingConfigString)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_uvmdanglingconfigstring extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

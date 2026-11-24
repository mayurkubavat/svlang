// Compliant Clean Test Case: SVL-OOP-008 (EmptyVirtualMethodWithoutPure)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_emptyvirtualmethodwithoutpure extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

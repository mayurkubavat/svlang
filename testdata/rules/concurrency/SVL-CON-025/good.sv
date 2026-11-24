// Compliant Clean Test Case: SVL-CON-025 (TryPutIgnoredReturn)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_tryputignoredreturn extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

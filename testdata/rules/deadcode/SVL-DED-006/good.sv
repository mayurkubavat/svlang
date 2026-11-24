// Compliant Clean Test Case: SVL-DED-006 (UnwrittenClassProperty)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_unwrittenclassproperty extends uvm_object;
    int items[$];
    function new(string name = "compliant");
        super.new(name);
    endfunction
    function int push_and_count(int v);
        items.push_back(v);
        return items.size();
    endfunction
endclass

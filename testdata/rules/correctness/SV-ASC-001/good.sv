// Compliant Clean Test Case: SV-ASC-001 (AssociativeArrayMutatingRead)
`include "uvm_mock.svh"

class compliant_associativearraymutatingread extends uvm_object;
    int fixed_arr[4];
    int assoc[string];

    function new(string name = "compliant");
        super.new(name);
    endfunction

    function int lookup(int idx, string key);
        int sum = fixed_arr[idx];
        if (assoc.exists(key)) begin
            sum += assoc[key];
        end
        return sum;
    endfunction
endclass

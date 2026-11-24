// Test Case: SVL-PRF-014 (AssociativeArrayMutatingRead)
// Extracted from sample_tb.sv (Test Case 126)

`include "uvm_mock.svh"

// --- TEST CASE 126: SVL-PRF-014 (AssociativeArrayMutatingRead) ---
class AssocReadBench;
    int scores[int];
    function bit check_score(int player_id);
        // VIOLATION SVL-PRF-014: Direct read without exists() mutates associative array!
        if (scores[player_id] == 100) begin
            return 1;
        end
        return 0;
    endfunction
endclass

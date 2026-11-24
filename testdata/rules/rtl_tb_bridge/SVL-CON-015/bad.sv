// Test Case: SVL-CON-015 (ClockingBlockOutputReadSkew)
// Extracted from sample_tb.sv (Test Case 127)

`include "uvm_mock.svh"

interface mock_bus_if;
    logic clk;
    logic [7:0] data;
    clocking cb @(posedge clk);
        output data;
    endclocking
endinterface

// --- TEST CASE 127: SVL-CON-015 (ClockingBlockOutputReadSkew) ---
class ClockingReadBench;
    virtual mock_bus_if vif;
    function bit inspect_output();
        // VIOLATION SVL-CON-015: Reading clocking block output signal!
        if (vif.cb.data == 8'h00) begin
            return 1;
        end
        return 0;
    endfunction
endclass

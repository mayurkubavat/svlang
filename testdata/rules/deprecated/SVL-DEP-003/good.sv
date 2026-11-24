// Compliant Clean Test Case: SVL-DEP-003 (LegacyAlwaysBlock)
`include "uvm_mock.svh"

module compliant_legacyalwaysblock(output logic q_o);
    logic clk = 1'b0;
    // IEEE 1800 testbench clock generator: cannot be expressed with always_ff / always_comb
    always #5ns clk = ~clk;

    always_ff @(posedge clk) begin
        q_o <= ~q_o;
    end
endmodule

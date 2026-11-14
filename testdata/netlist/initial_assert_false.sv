// A parameter check that fails for the elaborated parameters is an error, not a netlist.
module initial_assert_false #(
    parameter int DEPTH = 0
) (
    input  logic clk,
    input  logic d,
    output logic q
);
    initial assert (DEPTH > 0) else $fatal(1, "DEPTH must be positive");
    always_ff @(posedge clk) q <= d;
endmodule

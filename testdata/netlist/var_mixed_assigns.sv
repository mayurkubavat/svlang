// IEEE 1800 section 6.5: a variable is driven continuously or procedurally, not both.
module var_mixed_assigns (
    input  logic clk, a, b,
    output logic y
);
    logic v;
    assign v = a;
    always_ff @(posedge clk) v <= b;
    assign y = v;
endmodule

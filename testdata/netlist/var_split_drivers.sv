// Different bits of a variable may have different drivers, and a generate branch
// that is not taken does not count: one driver per bit is what matters.
module var_split_drivers #(
    parameter bit Invert = 0
) (
    input  logic       a, b,
    output logic [1:0] y,
    output logic       z
);
    logic [1:0] v;
    logic       w;
    assign v[0] = a;
    always_comb v[1] = b;
    if (Invert) begin : g_inv
        assign w = ~a;
    end else begin : g_plain
        assign w = a;
    end
    assign y = v;
    assign z = w;
endmodule

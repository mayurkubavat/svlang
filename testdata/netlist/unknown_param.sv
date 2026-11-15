// A parameter override must name a parameter the module has.
module unknown_param_leaf #(
    parameter int W = 2
) (
    input  logic [W-1:0] a,
    output logic [W-1:0] y
);
    assign y = ~a;
endmodule

module unknown_param (
    input  logic [3:0] a,
    output logic [3:0] y
);
    unknown_param_leaf #(.W(4), .Depth(8)) u_leaf (.a(a), .y(y));
endmodule

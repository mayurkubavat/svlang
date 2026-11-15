// 2**n with an unsigned signal exponent is a one shifted left by n.
module pow_of_two (
    input  logic [2:0]  n,
    input  logic [7:0]  addr,
    output logic [7:0]  onehot,
    output logic        in_range
);
    assign onehot   = 2**n;
    assign in_range = (addr & (2**n - 1)) == 8'd0;
endmodule

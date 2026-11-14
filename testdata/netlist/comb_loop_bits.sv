// Two bits of one vector that feed each other are a real loop, bit by bit.
module comb_loop_bits (
    input  logic       a,
    output logic [1:0] y
);
    logic [1:0] c;
    assign c[0] = c[1] ^ a;
    assign c[1] = c[0];
    assign y = c;
endmodule

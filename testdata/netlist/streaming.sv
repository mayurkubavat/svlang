// Streaming concatenation of packed values: bit reversal, byte swap, and constants.
module streaming (
    input  logic [7:0]  a,
    input  logic [3:0]  b,
    output logic [7:0]  rev, same,
    output logic [15:0] swap,
    output logic [11:0] both,
    output logic [5:0]  k4,
    output logic [15:0] kswap
);
    assign rev   = {<<{a}};
    assign same  = {>>{a}};
    assign swap  = {<<8{a, 4'h0, b}};
    assign both  = {<<{a, b}};
    assign k4    = {<<4{6'b11_0101}};
    assign kswap = {<<8{16'hABCD}};
endmodule

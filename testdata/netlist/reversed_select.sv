// A part-select runs the way the declaration does: [0:3] of a [3:0] vector is an error.
module reversed_select (
    input  logic [3:0] a,
    output logic [3:0] r
);
    assign r = a[0:3];
endmodule

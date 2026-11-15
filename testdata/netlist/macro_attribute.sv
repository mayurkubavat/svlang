// An attribute may come from a macro: (* keep *) written in a `define body is the
// same attribute as one written in place.
`define KEEP (* keep *)
`define NOTHING
module macro_attribute (
    input  logic clk, d,
    output logic q
);
    `KEEP logic a;
    `NOTHING logic b;
    always_ff @(posedge clk) begin
        a <= d;
        b <= a;
        q <= b;
    end
endmodule

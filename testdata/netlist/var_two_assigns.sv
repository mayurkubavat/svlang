// IEEE 1800 section 6.5: a variable takes at most one continuous assignment.
module var_two_assigns (
    input  logic a, b,
    output logic y
);
    logic v;
    assign v = a;
    assign v = b;
    assign y = v;
endmodule

// An instance of a module nobody defined must not vanish from the design.
module unknown_module (
    input  logic a,
    output logic y
);
    logic w;
    not_defined u_missing (.i(a), .o(w));
    assign y = w;
endmodule

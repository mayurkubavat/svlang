// A named connection must name a port the module has.
module unknown_port_leaf (
    input  logic a,
    output logic y
);
    assign y = ~a;
endmodule

module unknown_port (
    input  logic a,
    output logic y
);
    unknown_port_leaf u_leaf (.a(a), .y(y), .enable(a));
endmodule

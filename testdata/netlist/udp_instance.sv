// A user-defined primitive is not lowered; its instance must be reported.
primitive udp_mux (output o, input a, input b, input s);
    table
        0 ? 0 : 0;
        1 ? 0 : 1;
        ? 0 1 : 0;
        ? 1 1 : 1;
    endtable
endprimitive

module udp_instance (
    input  logic a, b, s,
    output logic o
);
    udp_mux u_mux (o, a, b, s);
endmodule

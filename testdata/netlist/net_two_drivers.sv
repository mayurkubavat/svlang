// Two drivers on a net are legal SystemVerilog (the net resolves them); the netlist
// does not model resolution, so this is unsupported, not an error.
module net_two_drivers (
    input  logic a, b,
    output logic y
);
    wire v;
    assign v = a;
    assign v = b;
    assign y = v;
endmodule

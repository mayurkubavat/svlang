// Bad: 8-bit signal connected to 4-bit formal port
module leaf_w4 (
  input  logic [3:0] in_d,
  output logic [3:0] out_q
);
  assign out_q = in_d;
endmodule

module bad_port_width (
  input  logic [7:0] wide_in,
  output logic [3:0] narrow_out
);
  leaf_w4 u_leaf (
    .in_d  (wide_in),
    .out_q (narrow_out)
  );
endmodule

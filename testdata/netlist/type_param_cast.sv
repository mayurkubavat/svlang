// A type parameter names a type in casts, with its default and with an override.
module type_param_leaf #(parameter type data_t = logic [3:0])(input data_t d, output data_t q);
  assign q = data_t'(d);
endmodule
module type_param_cast (
    input  logic [7:0] d,
    output logic [3:0] q4,
    output logic [7:0] q8,
    output logic [1:0] q2
);
    type_param_leaf                             u4 (.d(d[3:0]), .q(q4));
    type_param_leaf #(.data_t(logic [7:0]))     u8 (.d(d),      .q(q8));
    type_param_leaf #(.data_t(logic [1:0]))     u2 (.d(d[1:0]), .q(q2));
endmodule

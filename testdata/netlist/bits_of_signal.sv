// $bits of a signal, a struct member or a concatenation is a constant per instance:
// parameters, child parameter overrides and widths may depend on it.
package bits_pk; typedef struct packed { logic [2:0] op; logic [7:0] data; } t; endpackage
module buf_w #(parameter int Width = 1)(input [Width-1:0] i, output [Width-1:0] o); assign o = i; endmodule
module bits_leaf import bits_pk::*; #(parameter int N = 4)(input t x, input [N-1:0] y, output logic [31:0] o, output logic [N-1:0] o2);
  localparam int DW = $bits(x.data);
  localparam int CW = $bits({x.op, y, 1'b0});
  localparam int HW = DW/2;
  logic [CW-1:0] cat;
  buf_w #(.Width($bits({x.op, y, 1'b0}))) u(.i({x.op, y, 1'b0}), .o(cat));
  buf_w #(.Width($bits(y))) u2(.i(y), .o(o2));
  assign o = {{(32-CW-DW){1'b0}}, cat, x.data};
endmodule
module bits_of_signal(input bits_pk::t x, input [5:0] y, output [31:0] a, b, output [3:0] c, output [5:0] d);
  bits_leaf #(4) u4(.x(x), .y(y[3:0]), .o(a), .o2(c));
  bits_leaf #(6) u6(.x(x), .y(y), .o(b), .o2(d));
endmodule

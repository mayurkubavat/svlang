// A macro may be used inside its own argument: the argument is text of the use, not
// of the macro body, so this is not recursion.
`define MAX_OF(a,b) ((a) > (b) ? (a) : (b))
module nested_macro(output [7:0] y);
  localparam int W = `MAX_OF(3, `MAX_OF(5, 4));
  assign y = W;
endmodule

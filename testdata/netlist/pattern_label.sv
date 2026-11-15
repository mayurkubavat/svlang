// A member label in an assignment pattern is not a use of a signal declared later
// with the same name.
package pattern_label_pkg; typedef struct packed { logic [1:0] rev; logic [1:0] locality; } cap_t; endpackage
module pattern_label import pattern_label_pkg::*; (output cap_t y);
  assign y = '{
    rev: 2'h1,
    locality: 2'h2
  };
  logic [3:0] locality;
  assign locality = 4'b0;
endmodule

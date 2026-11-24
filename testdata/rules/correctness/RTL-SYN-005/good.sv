// Good: y has a complete default assignment on all paths in always_comb
module good_comb_complete (
  input  logic       sel,
  input  logic [7:0] a,
  output logic [7:0] y
);
  always_comb begin
    y = 8'd0;
    if (sel) begin
      y = a;
    end
  end
endmodule

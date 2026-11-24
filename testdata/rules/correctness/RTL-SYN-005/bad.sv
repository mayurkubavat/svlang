// Bad: y is only assigned when sel is high inside always_comb (infers latch)
module bad_comb_latch (
  input  logic       sel,
  input  logic [7:0] a,
  output logic [7:0] y
);
  always_comb begin
    if (sel) begin
      y = a;
    end
  end
endmodule

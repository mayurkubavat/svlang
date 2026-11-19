// Bad: always_ff clocked by combinationally gated AND net
module bad_comb_clk_gate (
  input  logic clk,
  input  logic en,
  input  logic d,
  output logic q
);
  logic gclk;
  assign gclk = clk & en;

  always_ff @(posedge gclk) begin
    q <= d;
  end
endmodule

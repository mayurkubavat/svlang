// Good: always_ff clocked directly by primary clock with synchronous enable
module good_sync_enable (
  input  logic clk,
  input  logic en,
  input  logic d,
  output logic q
);
  always_ff @(posedge clk) begin
    if (en) q <= d;
  end
endmodule

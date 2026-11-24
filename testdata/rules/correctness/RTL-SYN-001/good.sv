// Good: consistent active-low reset polarity across all sequential blocks
module good_consistent_reset (
  input  logic clk,
  input  logic rst_n,
  input  logic d,
  output logic q1,
  output logic q2
);
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) q1 <= 1'b0;
    else        q1 <= d;
  end

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) q2 <= 1'b0;
    else        q2 <= q1;
  end
endmodule

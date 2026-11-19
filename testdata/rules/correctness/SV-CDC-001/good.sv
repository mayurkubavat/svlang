// Good: 1-bit flag synchronized via dedicated 2-FF chain in clk_b
module good_cdc_2ff (
  input  logic clk_a,
  input  logic clk_b,
  input  logic req_in,
  output logic req_sync_o
);
  logic req_a_q;
  logic sync_q1, sync_q2;

  always_ff @(posedge clk_a) begin
    req_a_q <= req_in;
  end

  always_ff @(posedge clk_b) begin
    sync_q1 <= req_a_q;
    sync_q2 <= sync_q1;
  end

  assign req_sync_o = sync_q2;
endmodule

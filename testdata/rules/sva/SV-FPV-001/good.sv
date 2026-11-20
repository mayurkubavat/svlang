module fpv001_good (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       inc,
  input  logic [3:0] max_cfg,
  output logic       out_vld
);
  logic [3:0] cnt_q;
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) cnt_q <= 4'd0;
    else if (inc) cnt_q <= cnt_q + 4'd1;
  end
  assign out_vld = (cnt_q == 4'd7);

  m_env: assume property (@(posedge clk) disable iff (!rst_n) max_cfg < 4'd12);
  a_inv: assert property (@(posedge clk) disable iff (!rst_n) cnt_q <= 4'd15);
endmodule

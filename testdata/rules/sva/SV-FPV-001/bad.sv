module fpv001_bad (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       inc,
  output logic       out_vld
);
  logic [3:0] cnt_q;
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) cnt_q <= 4'd0;
    else if (inc) cnt_q <= cnt_q + 4'd1;
  end
  assign out_vld = (cnt_q == 4'd7);

  a_out: assume property (@(posedge clk) disable iff (!rst_n) !out_vld);
  a_st:  assume property (@(posedge clk) disable iff (!rst_n) cnt_q < 4'd8);
endmodule

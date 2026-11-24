module fpv002_bad (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       in_vld,
  input  logic [7:0] in_data,
  output logic       out_vld,
  output logic [7:0] out_data
);
  logic       vld_no_rst_q;
  logic [7:0] data_q;

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) data_q <= 8'd0;
    else if (vld_no_rst_q) data_q <= in_data;
  end

  always_ff @(posedge clk) begin
    vld_no_rst_q <= in_vld;
  end

  assign out_vld  = vld_no_rst_q;
  assign out_data = data_q;
endmodule

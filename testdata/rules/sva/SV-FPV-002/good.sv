module fpv002_good (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       in_vld,
  input  logic [7:0] in_data,
  output logic       out_vld,
  output logic [7:0] out_data
);
  logic       vld_q;
  logic [7:0] pipe_data_no_rst_q;

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) vld_q <= 1'b0;
    else        vld_q <= in_vld;
  end

  // Reset-less wide datapath pipe that does NOT feed 1-bit control/mux-sel/mem-wen is clean!
  always_ff @(posedge clk) begin
    pipe_data_no_rst_q <= in_data;
  end

  assign out_vld  = vld_q;
  assign out_data = pipe_data_no_rst_q;
endmodule

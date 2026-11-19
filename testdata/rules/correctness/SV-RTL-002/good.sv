// Good: unreset datapath register placed in separate clock-only always_ff
module good_split_reset (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       vld_i,
  input  logic [7:0] dat_i,
  output logic       valid_q,
  output logic [7:0] data_q
);
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) valid_q <= 1'b0;
    else        valid_q <= vld_i;
  end

  always_ff @(posedge clk) begin
    data_q <= dat_i;
  end
endmodule

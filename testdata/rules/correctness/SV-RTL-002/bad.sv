// Bad: async-reset always_ff resets valid_q but omits data_q in the same block
module bad_partial_reset (
  input  logic       clk,
  input  logic       rst_n,
  input  logic       vld_i,
  input  logic [7:0] dat_i,
  output logic       valid_q,
  output logic [7:0] data_q
);
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) begin
      valid_q <= 1'b0;
    end else begin
      valid_q <= vld_i;
      data_q  <= dat_i;
    end
  end
endmodule

// Bad: multi-bit bus sampled directly across asynchronous clocks clk_a -> clk_b
module bad_cdc_bus (
  input  logic       clk_a,
  input  logic       clk_b,
  input  logic [7:0] din,
  output logic [7:0] dout
);
  logic [7:0] src_data_q;

  always_ff @(posedge clk_a) begin
    src_data_q <= din;
  end

  always_ff @(posedge clk_b) begin
    dout <= src_data_q;
  end
endmodule

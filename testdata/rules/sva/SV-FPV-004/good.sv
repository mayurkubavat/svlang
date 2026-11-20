module fpv004_good (
  input  logic       clk,
  input  logic       rst_n,
  input  logic [3:0] a,
  input  logic [3:0] b,
  input  logic [1:0] addr,
  input  logic       wen,
  output logic [7:0] prod_o,
  output logic [7:0] rdata_o
);
  logic [7:0] small_ram [0:3];
  always_ff @(posedge clk) begin
    if (wen) small_ram[addr] <= {4'd0, a};
  end
  assign prod_o  = a * b;
  assign rdata_o = small_ram[addr];

  a_chk: assert property (@(posedge clk) disable iff (!rst_n) prod_o <= 8'd225);
endmodule

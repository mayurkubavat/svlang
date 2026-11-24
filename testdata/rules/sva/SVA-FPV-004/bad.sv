module fpv004_bad (
  input  logic        clk,
  input  logic        rst_n,
  input  logic [15:0] a,
  input  logic [15:0] b,
  input  logic [7:0]  addr,
  input  logic        wen,
  output logic [15:0] prod_o,
  output logic [15:0] rdata_o
);
  logic [15:0] big_ram [0:255];
  always_ff @(posedge clk) begin
    if (wen) big_ram[addr] <= a;
  end
  assign prod_o  = a * b;
  assign rdata_o = big_ram[addr];

  a_chk: assert property (@(posedge clk) disable iff (!rst_n) prod_o == (a * b));
endmodule

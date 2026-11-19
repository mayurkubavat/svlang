// Good: cover property uses ##1 sequence concatenation
module good_cover_sequence (
  input logic clk,
  input logic rst_n,
  input logic req,
  input logic gnt
);
  c_req_gnt: cover property (@(posedge clk) disable iff (!rst_n) req ##1 gnt);
endmodule

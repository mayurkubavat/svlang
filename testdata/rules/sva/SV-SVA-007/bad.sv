// Bad: cover property uses |-> implication, matching vacuously when req is 0
module bad_cover_implication (
  input logic clk,
  input logic rst_n,
  input logic req,
  input logic gnt
);
  c_req_gnt: cover property (@(posedge clk) disable iff (!rst_n) req |-> ##1 gnt);
endmodule

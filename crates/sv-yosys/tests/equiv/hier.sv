module add1 #(parameter int W = 4) (input logic [W-1:0] x, output logic [W-1:0] y);
  assign y = x + 1'b1;
endmodule
module hier (input logic clk, rst, input logic [5:0] d, output logic [5:0] q, output logic [2:0] sm);
  logic [5:0] n;
  add1 #(.W(6)) u_a (.x(q ^ d), .y(n));
  add1 #(.W(3)) u_b (.x(d[2:0]), .y(sm));
  always_ff @(posedge clk) if (rst) q <= 6'd1; else q <= n;
endmodule

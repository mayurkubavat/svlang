module aload (input logic clk, ld, input logic [3:0] v, d, output logic [3:0] q);
  always_ff @(posedge clk or posedge ld) if (ld) q <= v; else q <= d;
endmodule

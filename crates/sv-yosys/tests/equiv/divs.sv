module divs (input logic [7:0] a, b, input logic signed [7:0] sa, sb, output logic [7:0] q, r, output logic signed [7:0] sq, sr);
  assign q = a / b;
  assign r = a % b;
  assign sq = sa / sb;
  assign sr = sa % sb;
endmodule

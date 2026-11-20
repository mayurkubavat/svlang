`define SCALE_ADD(x, y) ((x) * 4 + (y))
module top;
  int v = `SCALE_ADD(1 + 2, 3);
endmodule

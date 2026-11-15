// Bits of one vector may feed other bits of it: a carry chain, a tree kept in one
// vector, two blocks that each drive a part. None of these is a combinational loop.
module comb_chain_gen(input [3:0] a, output [3:0] s);
  logic [3:0] c;
  assign c[0] = a[0];
  for (genvar i = 1; i < 4; i++) begin : g
    assign c[i] = c[i-1] & a[i];
  end
  assign s = c;
endmodule
module comb_chain_proc(input [3:0] a, output logic [3:0] s);
  logic [3:0] c;
  always_comb begin
    c[0] = a[0];
    for (int i = 1; i < 4; i++) c[i] = c[i-1] & a[i];
  end
  assign s = c;
endmodule
module comb_chain_packed(input [3:0] a, output logic [3:0] s);
  logic [1:0][1:0] c;
  assign c[0] = a[1:0];
  assign c[1] = c[0] ^ a[3:2];
  assign s = c;
endmodule
module comb_chain_blocks(input [3:0] a, output logic [3:0] s);
  logic [3:0] c;
  always_comb begin c[1:0] = a[1:0]; end
  always_comb begin c[3:2] = c[1:0] ^ a[3:2]; end
  assign s = c;
endmodule
// A read of several bits, each from a different driver.
module comb_chain_span(input [3:0] req, input en, output [3:0] ack);
  assign ack[0] = req[0] ? en : 1'b0;
  for (genvar i = 1; i < 4; i++) begin : g
    assign ack[i] = (req[i] & ~(|ack[i-1:0])) ? en : 1'b0;
  end
endmodule
// Fields of one struct assigned in separate blocks, one field read by the other block.
module comb_chain_struct(input logic a, b, c, output logic [1:0] y);
  typedef struct packed { logic x; logic z; } s_t;
  s_t s;
  always_comb begin
    s.x = a;
    if (c) s.x = b;
  end
  always_comb begin
    s.z = 1'b0;
    if (s.x) s.z = b;
  end
  assign y = s;
endmodule
module comb_chain (
    input  logic [3:0] a,
    output logic [3:0] s0, s1, s2, s3, s4,
    output logic [1:0] s5
);
    comb_chain_gen    u0 (.a(a), .s(s0));
    comb_chain_proc   u1 (.a(a), .s(s1));
    comb_chain_packed u2 (.a(a), .s(s2));
    comb_chain_blocks u3 (.a(a), .s(s3));
    comb_chain_span   u4 (.req(a), .en(1'b1), .ack(s4));
    comb_chain_struct u5 (.a(a[0]), .b(a[1]), .c(a[2]), .y(s5));
endmodule

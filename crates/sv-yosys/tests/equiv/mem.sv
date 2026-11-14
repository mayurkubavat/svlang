module mem (input logic clk, we, input logic [1:0] wa, ra, input logic [3:0] d, output logic [3:0] q);
  logic [3:0] m [4];
  always_ff @(posedge clk) begin
    if (we) m[wa] <= d;
    q <= m[ra];
  end
endmodule

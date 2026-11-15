// An array cleared word by word in the reset branch of an async-reset block is a set
// of registers with an asynchronous reset, not a memory.
module async_reset_array #(parameter int W = 8, parameter int D = 4)(
  input logic clk, rst_n, we, input logic [1:0] wa, ra, input logic [W-1:0] wd, output logic [W-1:0] rd);
  logic [W-1:0] ram_q [D];
  always_ff @(posedge clk or negedge rst_n) begin : p_ram
    if (!rst_n) begin
      for (int i = 0; i < D; i++) ram_q[i] <= '0;
    end else if (we) begin
      ram_q[wa] <= wd;
    end
  end : p_ram
  assign rd = ram_q[ra];
endmodule

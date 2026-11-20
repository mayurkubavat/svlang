module fpv003_good (
  input  logic clk,
  input  logic rst_n,
  input  logic req,
  output logic gnt
);
  default disable iff (!rst_n);

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) gnt <= 1'b0;
    else        gnt <= req;
  end

  a_with_default_dis: assert property (@(posedge clk) req |=> gnt);
endmodule

module fpv003_bad (
  input  logic clk,
  input  logic rst_n,
  input  logic req,
  output logic gnt
);
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) gnt <= 1'b0;
    else        gnt <= req;
  end

  a_no_dis: assert property (@(posedge clk) req |=> gnt);
endmodule

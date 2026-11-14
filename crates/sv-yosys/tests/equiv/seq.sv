module seq (input logic clk, rst, arst_n, en, input logic [7:0] d, output logic [7:0] q_sync, q_async, q_en, q_neg, output logic [3:0] cnt);
  always_ff @(posedge clk) if (rst) q_sync <= 8'd0; else q_sync <= d;
  always_ff @(posedge clk or negedge arst_n) if (!arst_n) q_async <= 8'h3C; else q_async <= d + q_sync;
  always_ff @(posedge clk) if (en) q_en <= d ^ q_en;
  always_ff @(negedge clk) q_neg <= q_sync;
  always_ff @(posedge clk) if (rst) cnt <= '0; else if (en && cnt != 4'd11) cnt <= cnt + 4'd1;
endmodule

module misc (input logic [7:0] a, input logic [9:0] big, input logic signed [3:0] s4, input logic [3:0] u4,
             output logic [7:0] y_shl_big, y_shr_big, output logic signed [7:0] y_ashr_big,
             output logic [11:0] y_zext, output logic signed [11:0] y_sext, y_mixed,
             output logic [7:0] y_pow, output logic [5:0] y_ones, output logic y_cmp_mixed, y_tern,
             output logic [7:0] y_part, output logic [3:0] y_idx, output logic [7:0] y_fn);
  function automatic logic [7:0] swap(input logic [7:0] x);
    swap = {x[3:0], x[7:4]};
  endfunction
  assign y_shl_big = a << big;
  assign y_shr_big = a >> big;
  assign y_ashr_big = $signed(a) >>> big;
  assign y_zext = u4;
  assign y_sext = s4;
  assign y_mixed = s4 + $signed(a);
`ifdef GOLD
  assign y_pow = a * a * a;
`else
  assign y_pow = a ** 3;
`endif
  assign y_cmp_mixed = s4 < $signed(u4);
  assign y_tern = a[0] ? (a > 8'd7) : (s4 == -4'sd2);
  assign y_part = a[big[2:0] +: 1] ? {a[3:0], u4} : {u4, a[7:4]};
  assign y_idx = a >> u4[1:0];
  assign y_fn = swap(a) ^ 8'h0F;
  always_comb begin
    y_ones = '0;
    for (int i = 0; i < 8; i++) y_ones += a[i];
  end
endmodule

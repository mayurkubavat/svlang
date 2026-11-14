module comb (input logic [7:0] a, b, input logic signed [7:0] sa, sb, input logic [2:0] sh, input logic [1:0] sel,
             output logic [7:0] y_add, y_sub, y_mul, y_and, y_or, y_xor, y_xnor, y_not, y_neg, y_shl, y_shr, y_mux,
             output logic signed [7:0] y_ashr, y_smul,
             output logic [15:0] y_cat, y_wide,
             output logic [3:0] y_slice,
             output logic y_eq, y_ne, y_lt, y_le, y_gt, y_ge, y_slt, y_sle, y_land, y_lor, y_lnot, y_rand, y_ror, y_rxor, y_wild,
             output logic signed [15:0] y_sext);
  assign y_add = a + b;
  assign y_sub = a - b;
  assign y_mul = a * b;
  assign y_smul = sa * sb;
  assign y_and = a & b;
  assign y_or = a | b;
  assign y_xor = a ^ b;
  assign y_xnor = a ~^ b;
  assign y_not = ~a;
  assign y_neg = -a;
  assign y_shl = a << sh;
  assign y_shr = a >> sh;
  assign y_ashr = sa >>> sh;
  assign y_cat = {a, b};
  assign y_wide = a * b + {8'd0, a};
  assign y_slice = a[6:3];
  assign y_eq = a == b;
  assign y_ne = a != b;
  assign y_lt = a < b;
  assign y_le = a <= b;
  assign y_gt = a > b;
  assign y_ge = a >= b;
  assign y_slt = sa < sb;
  assign y_sle = sa <= sb;
  assign y_land = a && b;
  assign y_lor = a || b;
  assign y_lnot = !a;
  assign y_rand = &a;
  assign y_ror = |a;
  assign y_rxor = ^a;
  assign y_sext = sa;
  always_comb begin
    case (sel)
      2'd0: y_mux = a;
      2'd1: y_mux = b;
      2'd2: y_mux = a + 8'd3;
      default: y_mux = 8'hA5;
    endcase
  end
  always_comb begin
    casez (a)
      8'b1???_0??1: y_wild = 1'b1;
      default: y_wild = 1'b0;
    endcase
  end
endmodule

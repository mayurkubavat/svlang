// A package parameter of an unpacked array type is a table: read by index, passed to
// a function, and used by the package's own functions.
package pkg_table_pkg;
  parameter logic [7:0] A2X [8] = '{8'h98, 8'hf3, 8'hf2, 8'h48, 8'h09, 8'h81, 8'ha9, 8'hff};
  parameter int Off [0:2] = '{3, 5, 7};
  // Matrix-vector multiplication in GF(2): c = A * b
  function automatic logic [7:0] mvm(logic [7:0] vec_b, logic [7:0] mat_a [8]);
    logic [7:0] vec_c;
    vec_c = '0;
    for (int i = 0; i < 8; i++) begin
      for (int j = 0; j < 8; j++) begin
        vec_c[i] = vec_c[i] ^ (mat_a[j][i] & vec_b[7-j]);
      end
    end
    return vec_c;
  endfunction
  function automatic logic [7:0] to_x(logic [7:0] v);
    return mvm(v, A2X);
  endfunction
endpackage
module package_table import pkg_table_pkg::*; (
    input  logic [7:0] d,
    input  logic [2:0] sel,
    output logic [7:0] x, y, row,
    output logic [31:0] off
);
    assign x   = pkg_table_pkg::to_x(d);
    assign y   = mvm(d, pkg_table_pkg::A2X);
    assign row = A2X[sel];
    assign off = Off[1];
endmodule

// A package subroutine binds its names in its package: its constants, types and
// sibling functions, and what the package imports, whatever the calling module
// declares or imports.
package pkg_scope_base_pkg;
  parameter int K = 3;
  typedef logic [K-1:0] k_t;
  function automatic k_t inc(k_t x); return x + 1'b1; endfunction
endpackage
package pkg_scope_pkg;
  import pkg_scope_base_pkg::*;
  parameter int W = 4;
  parameter logic [W-1:0] Mask = 4'h5;
  typedef enum logic [W-1:0] { True = 4'h6, False = 4'h9 } mubi_t;
  typedef struct packed { logic [W-1:0] a; logic b; } s_t;
  function automatic logic test_true(mubi_t val);
    return True == val;
  endfunction
  function automatic logic [W-1:0] get_a(s_t s);
    return helper(s.a) & Mask;
  endfunction
  function automatic logic [W-1:0] helper(logic [W-1:0] v);
    k_t low = inc(k_t'(v));
    return {v[W-1], low};
  endfunction
endpackage
// The module has signals named like the package's constants, and imports nothing.
module package_function_scope(input logic [3:0] Mask, input logic [7:0] W, input pkg_scope_pkg::mubi_t m, input pkg_scope_pkg::s_t s,
          output logic y, output logic [3:0] z);
  assign y = pkg_scope_pkg::test_true(m);
  assign z = pkg_scope_pkg::get_a(s) ^ Mask ^ W[3:0];
endmodule

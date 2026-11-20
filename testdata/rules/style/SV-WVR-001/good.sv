// Good: svlang-lint:off block waiver properly closed with svlang-lint:on
package good_waiver_pkg;
  // svlang-lint:off SV-STY-010
  localparam int A = 42;
  // svlang-lint:on SV-STY-010
endpackage : good_waiver_pkg

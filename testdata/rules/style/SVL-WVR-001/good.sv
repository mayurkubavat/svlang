// Good: svlang-lint:off block waiver properly closed with svlang-lint:on
package good_waiver_pkg;
  // svlang-lint:off SVL-STY-008
  localparam int A = 42;
  // svlang-lint:on SVL-STY-008
endpackage : good_waiver_pkg

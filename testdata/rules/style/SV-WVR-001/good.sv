// Good: uvmcheck:off block waiver properly closed with uvmcheck:on
package good_waiver_pkg;
  // uvmcheck:off SV-STY-010
  localparam int A = 42;
  // uvmcheck:on SV-STY-010
endpackage : good_waiver_pkg

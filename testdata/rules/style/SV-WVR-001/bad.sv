// Bad: uvmcheck:off block waiver opened and never closed before EOF
package bad_waiver_pkg;
  // uvmcheck:off SV-STY-010
  localparam int A = 42;
endpackage : bad_waiver_pkg

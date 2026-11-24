// Compliant Clean Test Case: SVL-DEP-007 (LegacyRegWireInTestbench)
`include "uvm_mock.svh"

// IEEE 1800-2023 §6.6/§6.7: 'wire' is the standard net type for multi-driver,
// inout, and continuous-assignment nets — never deprecated in favor of 'logic'.
module compliant_nets_and_logic(inout wire pad_io);
    wire [7:0] bus_net;
    logic [7:0] state_q;
endmodule

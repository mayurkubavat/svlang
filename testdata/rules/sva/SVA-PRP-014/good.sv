// Compliant Clean Test Case: SVA-PRP-014 (DisableIffReset)
`include "uvm_mock.svh"

module SvaResetGood1(input logic clk, input logic rst_n, input logic req, input logic ack);
    // Named property defines disable iff; assert property instantiation must NOT
    // add a second nested disable iff (illegal per IEEE 1800-2023 §16.12.1).
    property p_req_ack;
        @(posedge clk) disable iff (!rst_n) req |-> ##1 ack;
    endproperty
    assert property (p_req_ack);
endmodule

module SvaResetGood2(input logic clk, input logic rst_n, input logic req, input logic ack);
    // Module-level default disable iff covers all assertions in scope (§16.15).
    default disable iff (!rst_n);
    assert property (@(posedge clk) req |-> ##1 ack);
endmodule

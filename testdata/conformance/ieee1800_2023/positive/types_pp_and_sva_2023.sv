// IEEE 1800-2023 positive conformance: boolean `ifdef expressions, triple-quoted
// string literals, `union soft` (with and without `packed`), restricted type
// parameters, and SVA `not` / `#-#` / `#=#` / `accept_on` / `reject_on`.

`define FEAT_2023_A 1

`ifdef (FEAT_2023_A && !FEAT_2023_MISSING)
package pkg_2023;
    localparam string BANNER = """IEEE 1800-2023
multiline banner""";
    typedef enum logic [1:0] { ST_IDLE = 2'd0, ST_RUN = 2'd1 } state_e;
    typedef struct packed { logic [7:0] hi; logic [7:0] lo; } pair_s;
    typedef union soft {
        logic [15:0] full;
        logic [5:0]  low6;
    } soft_implicit_u;
    typedef union soft packed {
        logic [15:0] full;
        logic [7:0]  low8;
    } soft_explicit_u;
endpackage
`endif

class Holder2023;
endclass

module restricted_sub #(
    parameter type enum   E_T = pkg_2023::state_e,
    parameter type struct S_T = pkg_2023::pair_s,
    parameter type union  U_T = pkg_2023::soft_implicit_u,
    parameter type class  C_T = Holder2023
) (
    input  logic        clk,
    input  logic        rst,
    input  logic        req,
    input  logic        ack,
    input  logic        abt,
    input  logic [5:0]  narrow_in,
    output logic [15:0] wide_out
);
    U_T u_reg;
    always_comb begin
        u_reg = '0;
        u_reg.low6 = narrow_in;
    end
    assign wide_out = u_reg.full;

    a_not:  assert property (@(posedge clk) disable iff (rst) not (req ##1 !ack));
    a_fby0: assert property (@(posedge clk) disable iff (rst) req |-> ((req ##1 ack) #-# ack));
    a_fby1: assert property (@(posedge clk) disable iff (rst) req |-> (req #=# ack));
    a_acc:  assert property (@(posedge clk) disable iff (rst) accept_on (abt) (req |-> ##1 ack));
    a_rej:  assert property (@(posedge clk) disable iff (rst) sync_reject_on (abt) (req |-> ack));
endmodule

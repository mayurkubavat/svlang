module bus_leaf (
    inout  wire [7:0] dq,
    input  logic      oe,
    input  logic [7:0] tx,
    output logic [7:0] rx
);
    assign dq = oe ? tx : 8'bz;
    assign rx = dq;
endmodule

module inout_bus (
    input  logic       clk,
    input  logic       rst_n,
    inout  wire  [7:0] pad_io,
    input  logic       oe_a,
    input  logic [7:0] tx_a,
    input  logic       oe_b,
    input  logic [7:0] tx_b,
    output logic [7:0] rx_q,
    output logic [3:0] pull_lo,
    output logic [3:0] pull_hi
);
    wire [7:0] rx_a, rx_b;
    bus_leaf u_a (.dq(pad_io), .oe(oe_a), .tx(tx_a), .rx(rx_a));
    bus_leaf u_b (.dq(pad_io), .oe(oe_b), .tx(tx_b), .rx(rx_b));

    tri0 [3:0] t0_bus;
    tri1 [3:0] t1_bus;
    assign t0_bus = oe_a ? tx_a[3:0] : 4'bz;
    assign t1_bus = oe_b ? tx_b[3:0] : 4'bz;
    assign pull_lo = t0_bus;
    assign pull_hi = t1_bus;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) rx_q <= 8'h00;
        else        rx_q <= rx_a ^ rx_b;
    end
endmodule

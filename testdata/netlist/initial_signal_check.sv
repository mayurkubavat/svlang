// A time-zero assertion over a signal is not modeled: unsupported by default, left out
// with --skip-initial-checks. The constant check beside it is decided either way.
module initial_signal_check #(
    parameter int W = 4
) (
    input  logic         clk,
    input  logic [W-1:0] d,
    input  logic         connected,
    output logic [W-1:0] q
);
    initial begin
        #1ps;
        Connected_A: assert (connected === 1'b1) else $error("not connected");
    end
    initial assert (W > 0) else $fatal(1, "W must be positive");
    always_ff @(posedge clk) q <= d;
endmodule

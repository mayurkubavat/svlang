// An initial block that only checks and reports leaves no trace in the netlist:
// constant assertions are decided, display and severity tasks are dropped, and a
// delay in front of a check does not matter. `k` keeps the value the loop leaves.
module initial_checks #(
    parameter int W = 4
) (
    input  logic         clk,
    input  logic [W-1:0] d,
    output logic [W-1:0] q
);
    integer k;
    initial begin : p_checks
        assert (W > 0) else $fatal(1, "W must be positive");
        width_ok: assert (W <= 64) else begin
            $error("W is too wide");
        end
        assert (W != 4) else $warning("the default width");
        cover (W == 4);
        $display("initial_checks %m: W=%0d", W);
        for (k = 0; k < W; k = k + 1) begin
            if (k > 64) $error("unreachable");
        end
        #1 $info("after a delay");
        #0;
    end : p_checks

    always_ff @(posedge clk) begin : p_reg
        q <= d;
    end : p_reg

    always_comb begin : p_unused
        logic [W-1:0] t;
        t = d;
    end : p_unused
endmodule

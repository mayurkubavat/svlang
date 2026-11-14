// A static assertion written as an elaboration task may pass a bare label as a message
// argument (OpenTitan's ASSERT_INIT under FPV_ON). The label names nothing, and the
// task's branch is not taken, so it is not an undeclared identifier.
module elab_task_label #(
    parameter int W = 4,
    parameter int Lut [2] = '{1, 2}
) (
    input  logic [W-1:0] a,
    output logic [W-1:0] y
);
    if (!(W >= $low(Lut) + 1)) $fatal(2, "Fatal static assertion [%s]: (%s) is not true.",
                                      (WidthCheck_A), (W >= $low(Lut) + 1));
    assign y = a;
endmodule

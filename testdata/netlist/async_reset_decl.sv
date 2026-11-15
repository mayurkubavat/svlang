// A local declaration before the reset 'if' of an async-reset block is in scope in
// both branches.
module async_reset_decl (
    input  logic        clk, reset_n, we,
    input  logic [1:0]  wa,
    input  logic [7:0]  wd,
    output logic [7:0]  r0, r3
);
    logic [7:0] w_mem [0:3];
    always @(posedge clk or negedge reset_n)
      begin : reg_update
        integer ii;
        if (!reset_n)
          begin
            for (ii = 0; ii < 4; ii = ii + 1)
              w_mem[ii] <= 8'h0;
          end
        else
          begin
            for (ii = 0; ii < 4; ii = ii + 1)
              if (we && wa == ii) w_mem[ii] <= wd;
          end
      end
    assign r0 = w_mem[0];
    assign r3 = w_mem[3];
endmodule

// Good: all declared FSM states are reachable from IDLE
module good_fsm_reachable (
  input  logic clk,
  input  logic rst_n,
  input  logic go,
  output logic busy
);
  typedef enum logic [1:0] { IDLE = 2'd0, RUN = 2'd1, COOL = 2'd2 } state_e;
  state_e state_q, state_d;

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) state_q <= IDLE;
    else        state_q <= state_d;
  end

  always_comb begin
    state_d = state_q;
    unique case (state_q)
      IDLE: if (go) state_d = RUN;
      RUN:  state_d = COOL;
      COOL: state_d = IDLE;
      default: state_d = IDLE;
    endcase
  end

  assign busy = (state_q != IDLE);
endmodule

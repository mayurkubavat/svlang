// Good: WAIT_ACK exits back to IDLE when ack arrives
module good_fsm_no_deadlock (
  input  logic clk,
  input  logic rst_n,
  input  logic req,
  input  logic ack,
  output logic active
);
  typedef enum logic [1:0] { IDLE = 2'd0, WAIT_ACK = 2'd1 } state_e;
  state_e state_q, state_d;

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) state_q <= IDLE;
    else        state_q <= state_d;
  end

  always_comb begin
    state_d = state_q;
    unique case (state_q)
      IDLE:     if (req) state_d = WAIT_ACK;
      WAIT_ACK: if (ack) state_d = IDLE;
      default:  state_d = IDLE;
    endcase
  end

  assign active = (state_q == WAIT_ACK);
endmodule

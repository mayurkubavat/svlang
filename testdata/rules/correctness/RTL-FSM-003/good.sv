// Good: WAIT_ACK exits back to IDLE when ack arrives, and combinational enum
// decoders are not classified as FSM state machines.
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

module good_comb_opcode_decoder (
  input  logic [1:0] op_raw_i,
  output logic       alu_en_o
);
  typedef enum logic [1:0] { OP_NOP = 2'd0, OP_ADD = 2'd1, OP_SUB = 2'd2 } opcode_e;
  opcode_e opcode;
  assign opcode = opcode_e'(op_raw_i);

  always_comb begin
    alu_en_o = 1'b0;
    unique case (opcode)
      OP_ADD, OP_SUB: alu_en_o = 1'b1;
      default:        alu_en_o = 1'b0;
    endcase
  end
endmodule

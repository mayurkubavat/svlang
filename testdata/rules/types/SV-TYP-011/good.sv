module top;
  typedef enum logic [1:0] { IDLE = 2'd0, RUN = 2'd1, DONE = 2'd2 } state_e;
  state_e st;
  initial begin
    st = state_e'(2'd2);
  end
endmodule

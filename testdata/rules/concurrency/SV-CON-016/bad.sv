module top;
  event done_ev;
  initial begin
    fork
      begin
        @(done_ev);
      end
    join_none
    -> done_ev;
  end
endmodule

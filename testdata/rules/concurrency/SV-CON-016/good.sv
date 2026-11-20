module top;
  event done_ev;
  initial begin
    fork
      begin
        wait (done_ev.triggered);
      end
    join_none
    -> done_ev;
  end
endmodule

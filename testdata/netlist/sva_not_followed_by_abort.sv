module sva_not_followed_by_abort (
    input  logic clk,
    input  logic rst,
    input  logic abort,
    input  logic req,
    input  logic gnt,
    input  logic done
);
    default clocking @(posedge clk); endclocking
    default disable iff (rst);

    a_not_seq:    assert property (not (req ##1 gnt && done));
    a_not_imp:    assert property (not (req [->1] |-> !gnt));
    a_fby_ov:     assert property ((req ##1 gnt) #-# done);
    a_fby_nonov:  assert property (req [*0:1] #=# gnt);
    a_sync_acc:   assert property (sync_accept_on(abort) (req |-> gnt [->1] ##1 done));
    a_sync_rej:   assert property (sync_reject_on(abort) (req ##1 gnt));
    c_fby_ev:     cover  property ((req ##1 gnt) #-# s_eventually done);
endmodule

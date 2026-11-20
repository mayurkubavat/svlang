module top;
  int pkt_q[$];
  string tag;
  initial begin
    tag = "hello";
    pkt_q.push_back(1);
  end
endmodule

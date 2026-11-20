module fpv005_bad (
  input  logic clk,
  input  logic req,
  input  logic ack,
  output logic ok
);
  assign ok = req | ack;

  always_comb begin
    m_unsat: assume (req && !req);
    m_pos:   assume (ack);
    m_neg:   assume (!ack);
  end
endmodule

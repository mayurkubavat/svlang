module fpv005_good (
  input  logic clk,
  input  logic req,
  input  logic ack,
  output logic ok
);
  assign ok = req | ack;

  always_comb begin
    m_mutex: assume (!(req && ack));
  end
endmodule

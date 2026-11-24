module top;
  int sum;
  initial begin
    for (int i = 4; i >= 0; i--) begin
      sum += i;
    end
  end
endmodule

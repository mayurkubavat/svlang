// Good: every path through non-void function returns a value
class good_complete_return;
  function int decode(bit cond, int x);
    if (cond) begin
      return x + 1;
    end else begin
      return 0;
    end
  endfunction
endclass

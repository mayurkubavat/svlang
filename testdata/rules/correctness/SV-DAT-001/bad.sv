// Bad: non-void function falls off end when cond is false
class bad_missing_return;
  function int decode(bit cond, int x);
    if (cond) begin
      return x + 1;
    end
  endfunction
endclass

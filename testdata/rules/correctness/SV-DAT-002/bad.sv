// Bad: automatic local variable acc read before initialization
class bad_uninit_local;
  function int sum_pair(int a, int b);
    int acc;
    acc = acc + a + b;
    return acc;
  endfunction
endclass

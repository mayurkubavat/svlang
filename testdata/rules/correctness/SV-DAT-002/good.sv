// Good: automatic local variable acc initialized before read
class good_init_local;
  function int sum_pair(int a, int b);
    int acc = 0;
    acc = acc + a + b;
    return acc;
  endfunction
endclass

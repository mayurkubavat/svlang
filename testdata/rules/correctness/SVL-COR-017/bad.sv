// Bad: output argument err_o is never written in compute()
class bad_unwritten_output;
  function void compute(input int a, output int res_o, output bit err_o);
    res_o = a * 2;
  endfunction
endclass

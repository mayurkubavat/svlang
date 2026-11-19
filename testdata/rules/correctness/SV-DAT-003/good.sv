// Good: all output arguments are assigned in compute()
class good_written_output;
  function void compute(input int a, output int res_o, output bit err_o);
    res_o = a * 2;
    err_o = (a < 0);
  endfunction
endclass

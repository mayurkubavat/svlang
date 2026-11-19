// Good: constructor helper only invokes non-virtual initialization routines
class good_base_comp;
  int cfg_val;

  function new();
    init_defaults();
  endfunction

  function void init_defaults();
    cfg_val = 1;
  endfunction

  virtual function void reset_regs();
  endfunction
endclass

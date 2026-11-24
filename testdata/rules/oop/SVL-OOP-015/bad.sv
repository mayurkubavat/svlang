// Bad: constructor calls helper init_hw() which transitively calls virtual reset_regs()
class bad_base_comp;
  function new();
    init_hw();
  endfunction

  function void init_hw();
    reset_regs();
  endfunction

  virtual function void reset_regs();
  endfunction
endclass

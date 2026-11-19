// Bad: virtual override changes default argument value from 1 to 0
class base_xactor;
  virtual function void reset_state(bit hard = 1'b1);
  endfunction
endclass

class bad_derived_xactor extends base_xactor;
  virtual function void reset_state(bit hard = 1'b0);
  endfunction
endclass

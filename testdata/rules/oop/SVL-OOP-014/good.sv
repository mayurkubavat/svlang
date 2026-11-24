// Good: virtual override preserves exact base default argument value
class base_xactor;
  virtual function void reset_state(bit hard = 1'b1);
  endfunction
endclass

class good_derived_xactor extends base_xactor;
  virtual function void reset_state(bit hard = 1'b1);
  endfunction
endclass

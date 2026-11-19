// Good: disable soft targets variable with a soft constraint in base class
class base_pkt;
  rand bit [7:0] len;
  constraint len_soft_c { soft len == 8'd4; }
endclass

class good_disable_soft extends base_pkt;
  constraint override_c {
    disable soft len;
    len inside {[8'd8 : 8'd32]};
  }
endclass

// Bad: disable soft targets variable with no soft constraint in hierarchy
class base_pkt;
  rand bit [7:0] len;
  rand bit [7:0] tag;
  constraint len_soft_c { soft len == 8'd4; }
endclass

class bad_disable_soft extends base_pkt;
  constraint override_c {
    disable soft tag;
    tag == 8'd9;
  }
endclass

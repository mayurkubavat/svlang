// Good: acyclic strict ordering across constraint variables
class good_rel_cycle;
  rand bit [7:0] a;
  rand bit [7:0] b;
  rand bit [7:0] c;

  constraint order_c {
    a < b;
    b <= c;
  }
endclass

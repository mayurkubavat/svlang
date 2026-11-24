// Bad: strict relational cycle across constraint variables (a < b && b < a)
class bad_rel_cycle;
  rand bit [7:0] a;
  rand bit [7:0] b;
  rand bit [7:0] c;

  constraint order_c {
    a < b;
    b <= c;
    c <= a;
  }
endclass

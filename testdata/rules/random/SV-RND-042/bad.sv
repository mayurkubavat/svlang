// Bad: foreach over rand dynamic array without constraining .size()
class bad_foreach_unsized;
  rand bit [7:0] payload[];

  constraint elem_c {
    foreach (payload[i]) {
      payload[i] inside {[8'h10 : 8'h7f]};
    }
  }
endclass

// Good: .size() is constrained alongside foreach
class good_foreach_sized;
  rand bit [7:0] payload[];

  constraint size_c {
    payload.size() inside {[1 : 16]};
  }

  constraint elem_c {
    foreach (payload[i]) {
      payload[i] inside {[8'h10 : 8'h7f]};
    }
  }
endclass

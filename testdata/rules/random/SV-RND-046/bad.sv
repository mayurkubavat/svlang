class burst_item;
  rand bit [7:0] len;
  constraint default_len_c {
    soft len == 8'd4;
  }
  constraint fixed_len_c {
    len == 8'd8;
  }
endclass

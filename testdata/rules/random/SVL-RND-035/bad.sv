// Bad: mutating rand_mode inside post_randomize()
class bad_post_rand_mode;
  rand bit [7:0] addr;
  rand bit [7:0] data;

  function void post_randomize();
    addr.rand_mode(0);
  endfunction
endclass

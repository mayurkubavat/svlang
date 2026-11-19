// Good: configure rand_mode in pre_randomize(), only query in post_randomize()
class good_post_rand_mode;
  rand bit [7:0] addr;
  bit lock_addr;
  int was_rand;

  function void pre_randomize();
    if (lock_addr) addr.rand_mode(0);
  endfunction

  function void post_randomize();
    was_rand = addr.rand_mode();
  endfunction
endclass

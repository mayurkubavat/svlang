class packet_cfg;
  int timeout;
  function new(); timeout = 10; endfunction
  function void reset(); timeout = 0; endfunction
  int late_field;
endclass

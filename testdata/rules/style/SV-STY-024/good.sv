class packet_cfg;
  int timeout;
  int late_field;
  function new(); timeout = 10; endfunction
  function void reset(); timeout = 0; endfunction
endclass

class chan_cov;
  bit [3:0] op;
  covergroup op_cg;
    option.per_instance = 0;
    cp_op: coverpoint op;
  endgroup
  function new(); op_cg = new(); endfunction
endclass

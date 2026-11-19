// Good: non-overlapping uvm_reg_field bit slices [3:0] and [7:4]
`include "uvm_mock.svh"
`ifndef UVM_MACROS_SVH
class uvm_reg_field extends uvm_object;
  function new(string name = ""); super.new(name); endfunction
  function void configure(uvm_reg parent, int unsigned size, int unsigned lsb_pos,
                          string access, bit volatile, logic [63:0] reset,
                          bit has_reset, bit is_rand, bit individually_accessible);
  endfunction
endclass
`endif

class ctrl_reg extends uvm_reg;
  uvm_reg_field mode_f;
  uvm_reg_field status_f;

  function new(string name = "ctrl_reg");
    super.new(name);
  endfunction

  virtual function void build();
    mode_f = new("mode_f");
    status_f = new("status_f");
    mode_f.configure(this, 4, 0, "RW", 0, 64'h0, 1, 1, 0);
    status_f.configure(this, 4, 4, "RO", 0, 64'h0, 1, 0, 0);
  endfunction
endclass

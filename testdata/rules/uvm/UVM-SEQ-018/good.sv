`include "uvm_mock.svh"
class guarded_sp_seq extends uvm_sequence #(uvm_sequence_item);
    `uvm_object_utils(guarded_sp_seq)
    function new(string name = "guarded_sp_seq");
        super.new(name);
    endfunction : new
    task body();
        if (starting_phase != null) starting_phase.raise_objection(this, "seq start");
        if (starting_phase != null) starting_phase.drop_objection(this, "seq done");
    endtask : body
endclass : guarded_sp_seq

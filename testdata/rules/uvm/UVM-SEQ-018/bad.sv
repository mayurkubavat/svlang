`include "uvm_mock.svh"
class unguarded_sp_seq extends uvm_sequence #(uvm_sequence_item);
    `uvm_object_utils(unguarded_sp_seq)
    function new(string name = "unguarded_sp_seq");
        super.new(name);
    endfunction : new
    task body();
        starting_phase.raise_objection(this, "seq start");
        starting_phase.drop_objection(this, "seq done");
    endtask : body
endclass : unguarded_sp_seq

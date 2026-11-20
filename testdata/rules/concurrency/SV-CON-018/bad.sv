`timescale 1ns/1ps
class ref_fork_helper;
    task launch_bg(ref int status_out);
        fork
            #5 status_out = 1;
        join_none
    endtask : launch_bg
endclass : ref_fork_helper

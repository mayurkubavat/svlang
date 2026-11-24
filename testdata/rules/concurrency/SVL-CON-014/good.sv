`timescale 1ns/1ps
class ref_fork_helper;
    task launch_sync(ref int status_out);
        fork
            #5 status_out = 1;
        join
    endtask : launch_sync
endclass : ref_fork_helper

// Test Case: SVL-SEC-002 (UncheckedFopen)
// Extracted from sample_tb.sv (Test Case 110)

`include "uvm_mock.svh"

// --- TEST CASE 110: SVL-SEC-002 (UncheckedFopen) ---
class UncheckedFopenBench;
    function void write_log(string msg);
        int fd;
        // VIOLATION SVL-SEC-002: File descriptor not verified against 0!
        fd = $fopen("sim_output.log", "w");
        $fwrite(fd, "Log: %s\n", msg);
        $fclose(fd);
    endfunction
endclass

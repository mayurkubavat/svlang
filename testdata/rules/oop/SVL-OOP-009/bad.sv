// Test Case: SVL-OOP-009 (DowncastWithoutDynamicCast)
// Extracted from sample_tb.sv (Test Case 180)

`include "uvm_mock.svh"

// --- TEST CASE 180: SVL-OOP-009 (DowncastWithoutDynamicCast) ---
class BasePacket;
    int id;
endclass
class ExtendedPacket extends BasePacket;
    int payload;
endclass
class StaticCastClassBench;
    task test_cast();
        BasePacket b_pkt = new();
        ExtendedPacket e_pkt;
        // VIOLATION SVL-OOP-009: Static cast syntax on class handle instead of $cast()!
        e_pkt = ExtendedPacket'(b_pkt);
    endtask
endclass

// Test Case: SVL-PRF-006 (StringKeyAssociativeArray)
// Extracted from sample_tb.sv (Test Case 116)

`include "uvm_mock.svh"

// --- TEST CASE 116: SVL-PRF-006 (StringKeyAssociativeArray) ---
class StringKeyBench;
    // VIOLATION SVL-PRF-006: Associative array indexed by string!
    int packet_counter[string];
endclass

// VIOLATION SVL-PRF-006: KEY is string in name_pool, so pool is string-keyed
// (reported once, however many specializations make it so).
class key_pool #(type KEY = int, type T = int);
    T pool[KEY];
endclass : key_pool

class name_pool #(type T = int) extends key_pool #(string, T);
endclass : name_pool

class pool_user;
    name_pool #(byte) a;
    name_pool #(int) b;
endclass : pool_user

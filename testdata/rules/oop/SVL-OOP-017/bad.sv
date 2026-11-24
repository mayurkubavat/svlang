// Test Case: SVL-OOP-017 (MutableObjectGetterLeak)
// Extracted from sample_tb.sv (Test Case 135)

`include "uvm_mock.svh"

// --- TEST CASE 135: SVL-OOP-017 (MutableObjectGetterLeak) ---
class MockConfig;
    int timeout;
endclass

class EncapsulationBench;
    MockConfig m_cfg;
    // VIOLATION SVL-OOP-017: Getter leaks mutable internal object handle directly!
    function MockConfig get_cfg();
        return m_cfg;
    endfunction
endclass

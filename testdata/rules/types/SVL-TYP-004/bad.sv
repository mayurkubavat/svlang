// Test Case: SVL-TYP-004 (TaskWithNonVoidReturnType)
// Extracted from sample_tb.sv (Test Case 219)

`include "uvm_mock.svh"

// --- TEST CASE 219: SVL-TYP-004 (TaskWithNonVoidReturnType) ---
class TaskReturnTypeBench;
    // VIOLATION SVL-TYP-004: Task declared with non-void return type 'int'!
    task int run_computation();
        #10;
        return 0;
    endtask : run_computation
endclass : TaskReturnTypeBench

# sv-tests conformance

svlang on the tests of [sv-tests](https://github.com/chipsalliance/sv-tests), the public SystemVerilog conformance suite
(`tools/quality/sv_tests.py`). svlang `7faa84249e69`, 2026-11-15, sv-tests `415ba5f934ef`.
Regenerate after a fix; the suite is kept outside the repository.

1028 tests in the suite's `tests` directory, 1024 run: **1024 pass (100.0%)**, 0 positive tests wrongly rejected, 0 negative tests wrongly accepted, 0 crashes, 0 timeouts.
4 not run: 4 simulation only.

`svlang check` runs each test through preprocessing, parsing, the semantic checks and elaboration; a test
of the preprocessor alone (`:type: preprocessing`) is run with `svlang preprocess`. A positive test passes
when no error is reported, a negative test (`:should_fail_because:`) when one is. The suite also generates
tests from other projects with its own scripts; those are not part of this run.

## By clause

| clause | | tests run | pass | false reject | false accept | crash or timeout | pass rate |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | Lexical conventions | 50 | 50 | 0 | 0 | 0 | 100% |
| 6 | Data types | 84 | 84 | 0 | 0 | 0 | 100% |
| 7 | Aggregate data types | 104 | 104 | 0 | 0 | 0 | 100% |
| 8 | Classes | 53 | 53 | 0 | 0 | 0 | 100% |
| 9 | Processes | 46 | 46 | 0 | 0 | 0 | 100% |
| 10 | Assignment statements | 10 | 10 | 0 | 0 | 0 | 100% |
| 11 | Operators and expressions | 87 | 87 | 0 | 0 | 0 | 100% |
| 12 | Procedural programming statements | 27 | 27 | 0 | 0 | 0 | 100% |
| 13 | Tasks and functions | 15 | 15 | 0 | 0 | 0 | 100% |
| 14 | Clocking blocks | 5 | 5 | 0 | 0 | 0 | 100% |
| 15 | Interprocess synchronization | 5 | 5 | 0 | 0 | 0 | 100% |
| 16 | Assertions | 49 | 49 | 0 | 0 | 0 | 100% |
| 18 | Constrained random value generation | 134 | 134 | 0 | 0 | 0 | 100% |
| 20 | Utility system tasks and functions | 47 | 47 | 0 | 0 | 0 | 100% |
| 21 | Input/output system tasks | 29 | 29 | 0 | 0 | 0 | 100% |
| 22 | Compiler directives | 75 | 75 | 0 | 0 | 0 | 100% |
| 23 | Modules and hierarchy | 3 | 3 | 0 | 0 | 0 | 100% |
| 24 | Programs | 1 | 1 | 0 | 0 | 0 | 100% |
| 25 | Interfaces | 1 | 1 | 0 | 0 | 0 | 100% |
| 26 | Packages | 2 | 2 | 0 | 0 | 0 | 100% |
| generic | Suite's own generic tests | 184 | 184 | 0 | 0 | 0 | 100% |
| sanity.sv | Suite's sanity test | 1 | 1 | 0 | 0 | 0 | 100% |
| testbenches | UVM testbenches | 11 | 11 | 0 | 0 | 0 | 100% |
| uvm | UVM library | 1 | 1 | 0 | 0 | 0 | 100% |
| **all** |  | 1024 | 1024 | 0 | 0 | 0 | 100% |

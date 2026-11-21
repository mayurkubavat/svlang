cmake_minimum_required(VERSION 3.28...3.31)
project(svlang_oracles VERSION 0.3.0 LANGUAGES CXX)

# Optional dev-only Slang compiler dumpers (tools/oracles/*.cpp, ~880 LOC) used
# by differential conformance gates in tools/gates/. Nothing here is shipped,
# tested by default, or required to build or run `svlang`.
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_program(CCACHE_PROGRAM ccache)
if(CCACHE_PROGRAM)
    message(STATUS "Using ccache: ${CCACHE_PROGRAM}")
    set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
endif()

# Bound parallel C++20 compile RAM on laptops.
set_property(GLOBAL PROPERTY JOB_POOLS compile_job_pool=4 link_job_pool=1)
set(CMAKE_JOB_POOL_COMPILE compile_job_pool)
set(CMAKE_JOB_POOL_LINK link_job_pool)

if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|AppleClang|GNU")
    add_compile_options(-Wall -Wextra -Wpedantic -Wno-unused-parameter $<$<COMPILE_LANGUAGE:CXX>:-fvisibility-inlines-hidden>)
endif()

# Upstream Slang (pinned, fetched at configure time; see third_party/slang/README.md).
include(third_party/slang/slang.cmake)

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR})

# Reference dump of Slang's preprocessor (crates/sv-preprocessor, tools/gates/pp_parity.py).
add_executable(svlang_pp_dump tools/oracles/pp_dump.cpp)
target_link_libraries(svlang_pp_dump PRIVATE slang::slang)

# Reference syntax outline and expressions (crates/sv-syntax, tools/gates/cst_parity.py).
add_executable(svlang_cst_outline tools/oracles/cst_outline.cpp)
target_link_libraries(svlang_cst_outline PRIVATE slang::slang)
add_executable(svlang_cst_exprs tools/oracles/cst_exprs.cpp)
target_link_libraries(svlang_cst_exprs PRIVATE slang::slang)

# Reference symbols, types, values and hierarchy (crates/sv-semantic, tools/gates/sym_gate.py).
add_executable(svlang_sym_dump tools/oracles/sym_dump.cpp)
target_link_libraries(svlang_sym_dump PRIVATE slang::slang)

# Reference compile acceptance/rejection (tools/gates/{ieee_gate,slang_unit_gate}.py).
add_executable(svlang_compile_check tools/oracles/compile_check.cpp)
target_link_libraries(svlang_compile_check PRIVATE slang::slang)

# Reference std::regex answers (crates/svlang-lint, tools/gates/regex_gate.py).
add_executable(svlang_regexcheck tools/oracles/regexcheck.cpp)

# Reference expression types (crates/svlang-lint, tools/gates/expr_gate.py).
add_executable(svlang_exprdump tools/oracles/exprdump.cpp)
target_link_libraries(svlang_exprdump PRIVATE slang::slang)

# Reference generic class specialization counts (crates/sv-semantic, tools/gates/spec_gate.py).
add_executable(svlang_specdump tools/oracles/specdump.cpp)
target_link_libraries(svlang_specdump PRIVATE slang::slang)

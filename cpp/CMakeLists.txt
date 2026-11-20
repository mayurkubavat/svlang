# The Slang-based svlang-lint engine: libsvlang_lint, the svlang-lint CLI, the optional
# libsvlang_lint_model shared library (C ABI) and their tests. Included from the
# top-level CMakeLists.txt, which fetches Slang and sets the compile options.
# Sources and headers are relative to this folder; shared inputs (testdata/,
# data/, tools/, python/) are relative to the repository root.

# Core library sources
file(GLOB_RECURSE SVLANG_LINT_SOURCES
    src/core/*.cpp
    src/engine/*.cpp
    src/uvm/*.cpp
    src/rules/*.cpp
    src/waivers/*.cpp
    src/reporting/*.cpp
    src/style/*.cpp
    src/plugins/*.cpp
    src/lsp/*.cpp
    src/compat/*.cpp
    src/model/*.cpp
)

find_package(Python3 COMPONENTS Interpreter REQUIRED)
# --explain data: each rule's tested bad/good example and its options, generated from
# testdata/rules/ and the rule sources so the examples shown are the ones the tests run.
file(GLOB_RECURSE SVLANG_LINT_RULE_EXAMPLES CONFIGURE_DEPENDS
    ${PROJECT_SOURCE_DIR}/testdata/rules/*.sv)
file(GLOB_RECURSE SVLANG_LINT_RULE_SOURCES CONFIGURE_DEPENDS
    ${CMAKE_CURRENT_SOURCE_DIR}/src/rules/*.cpp ${CMAKE_CURRENT_SOURCE_DIR}/src/style/*.cpp)
set(SVLANG_LINT_RULE_EXTRAS ${CMAKE_CURRENT_BINARY_DIR}/generated/RuleExtras.cpp)
add_custom_command(
    OUTPUT ${SVLANG_LINT_RULE_EXTRAS}
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tools/codegen/gen_rule_extras.py
            --repo ${PROJECT_SOURCE_DIR} --out ${SVLANG_LINT_RULE_EXTRAS}
    DEPENDS ${PROJECT_SOURCE_DIR}/tools/codegen/gen_rule_extras.py ${SVLANG_LINT_RULE_EXAMPLES} ${SVLANG_LINT_RULE_SOURCES}
    COMMENT "Generating rule examples for --explain"
    VERBATIM)

# UVM API packs (data/uvm-api/<version>.json, made by tools/codegen/gen_uvm_api_pack.py from a
# uvm-core checkout), compiled into the binary for uvm::ApiPack.
file(GLOB SVLANG_LINT_UVM_API_PACKS CONFIGURE_DEPENDS ${PROJECT_SOURCE_DIR}/data/uvm-api/*.json)
set(SVLANG_LINT_UVM_API_PACKS_CPP ${CMAKE_CURRENT_BINARY_DIR}/generated/UvmApiPacks.cpp)
add_custom_command(
    OUTPUT ${SVLANG_LINT_UVM_API_PACKS_CPP}
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tools/codegen/embed_uvm_api_packs.py
            --packs ${PROJECT_SOURCE_DIR}/data/uvm-api --out ${SVLANG_LINT_UVM_API_PACKS_CPP}
    DEPENDS ${PROJECT_SOURCE_DIR}/tools/codegen/embed_uvm_api_packs.py ${SVLANG_LINT_UVM_API_PACKS}
    COMMENT "Embedding UVM API packs"
    VERBATIM)

add_library(libsvlang_lint STATIC ${SVLANG_LINT_SOURCES} ${SVLANG_LINT_RULE_EXTRAS} ${SVLANG_LINT_UVM_API_PACKS_CPP})
target_include_directories(libsvlang_lint PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
)
target_link_libraries(libsvlang_lint PUBLIC slang::slang)
# Fallback location of the Python SDK (config loader, plugin runner) for in-tree builds
target_compile_definitions(libsvlang_lint PRIVATE
    SVLANG_LINT_PYTHON_SDK_DIR="${PROJECT_SOURCE_DIR}/python")

# Precompile heavy C++20 and Slang AST headers to avoid re-parsing them 237 times
target_precompile_headers(libsvlang_lint PRIVATE
    <string>
    <vector>
    <memory>
    <string_view>
    <unordered_map>
    <variant>
    <optional>
    <slang/ast/ASTVisitor.h>
    <slang/syntax/AllSyntax.h>
    <slang/ast/Compilation.h>
    <svlang_lint/rules/Rule.h>
    <svlang_lint/rules/RuleRegistry.h>
    <svlang_lint/core/Diagnostic.h>
)

# Optional Unity build for accelerating full rebuilds with bounded batch size
option(SVLANG_LINT_ENABLE_UNITY_BUILD "Enable Unity build for rules" OFF)
if(SVLANG_LINT_ENABLE_UNITY_BUILD)
    set_target_properties(libsvlang_lint PROPERTIES
        UNITY_BUILD ON
        UNITY_BUILD_BATCH_SIZE 8
    )
endif()

# CLI executable
add_executable(svlang-lint src/main.cpp)
# Rules register themselves from static initializers in their own files, so the whole
# library is linked in; otherwise the linker drops rule objects nothing references.
target_link_libraries(svlang-lint PRIVATE "$<LINK_LIBRARY:WHOLE_ARCHIVE,libsvlang_lint>" slang::slang)
set_target_properties(svlang-lint PROPERTIES OUTPUT_NAME "svlang-lint")

# libsvlang_lint_model: the slang-free design model (svlang_lint::model) behind a C
# ABI, for Rust (crates/), Python and other DV tools. Only the C API is exported.
if(SVLANG_LINT_BUILD_MODEL_LIB)
    add_library(svlang_lint_model SHARED src/capi/CApi.cpp)
    target_link_libraries(svlang_lint_model PRIVATE libsvlang_lint slang::slang)
    set_target_properties(svlang_lint_model PROPERTIES
        CXX_VISIBILITY_PRESET hidden
        VISIBILITY_INLINES_HIDDEN ON)
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        # Keep slang's and the linter's symbols private to the library.
        target_link_options(svlang_lint_model PRIVATE "LINKER:--exclude-libs,ALL")
    endif()
endif()

# CTest Integration (enable_testing() is in the top-level CMakeLists.txt)
add_test(
    NAME rule_regression_suite
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/regression.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME all_rules_stub_suite
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/regression.py --binary $<TARGET_FILE:svlang-lint>
            --mode all-rules --baseline ${PROJECT_SOURCE_DIR}/cpp/tests/baselines/baseline_stub.json
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)

# Runs against the real Accellera library when a uvm-core checkout is given,
# e.g. cmake -DSVLANG_LINT_UVM_HOME=/path/to/uvm-core
set(SVLANG_LINT_UVM_HOME "" CACHE PATH "uvm-core checkout used by the real-UVM rule tests")
if(SVLANG_LINT_UVM_HOME)
    add_test(
        NAME all_rules_uvm_core_suite
        COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/regression.py --binary $<TARGET_FILE:svlang-lint>
                --mode all-rules --uvm-home ${SVLANG_LINT_UVM_HOME}
                --baseline ${PROJECT_SOURCE_DIR}/cpp/tests/baselines/baseline_uvm_core.json
        WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
    )
endif()
add_test(
    NAME project_config_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_config.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME slang_overlay_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_compat.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME lrm_conformance_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_conformance.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME ast_coverage_floor_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tools/quality/ast_coverage.py
            --slang-include ${slang_SOURCE_DIR}/include
            --floor ${PROJECT_SOURCE_DIR}/testdata/conformance/ast_coverage_floor.json
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME config_sweep_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_config_sweep.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME vendor_probes_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_vendor_probes.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME methodology_guide_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_guidelines.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME rule_catalog_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_catalog.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME waiver_hygiene_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_waivers.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME changed_since_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_changed.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME fix_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_fix.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME fix_invariants_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_fix_invariants.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME result_cache_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_cache.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME report_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_report.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME precision_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_precision.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME testbench_map_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_map.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME explain_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_explain.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME init_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_init.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME findings_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_findings.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME baseline_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_baseline.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME engine_gate_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_engine_gate.py
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME python_rules_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/python/tests/test_python_rules.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME history_rules_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_history.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME engine_equivalence
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/engine_equivalence.py --binary $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)
add_test(
    NAME lsp_integration_test
    COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/cpp/tests/test_lsp.py $<TARGET_FILE:svlang-lint>
    WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
)


if(SVLANG_LINT_BUILD_MODEL_LIB)
    # The model's C ABI, exercised from a C program.
    enable_language(C)
    add_executable(model_capi_smoke tests/capi_smoke.c)
    target_include_directories(model_capi_smoke PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
    target_link_libraries(model_capi_smoke PRIVATE svlang_lint_model)
    add_test(
        NAME model_capi_test
        COMMAND model_capi_smoke ${PROJECT_SOURCE_DIR}/testdata/common
                ${PROJECT_SOURCE_DIR}/testdata/rules/oop/SV-OOP-007/bad.sv
                ${PROJECT_SOURCE_DIR}/testdata/model/facts.sv
        WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
    )
    # The Python binding (python/svlang_lint/model.py) over the same ABI.
    add_test(
        NAME model_python_test
        COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/python/tests/test_model_python.py
                --lib $<TARGET_FILE:svlang_lint_model>
        WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
    )
    # The Rust crates (crates/) against this build's library, when cargo is around.
    find_program(CARGO_PROGRAM cargo)
    if(CARGO_PROGRAM)
        add_test(
            NAME rust_model_test
            COMMAND ${CMAKE_COMMAND} -E env SVLANG_LINT_BUILD_DIR=${CMAKE_BINARY_DIR}
                    ${CARGO_PROGRAM} test --quiet --workspace --features slang --manifest-path ${PROJECT_SOURCE_DIR}/Cargo.toml
            WORKING_DIRECTORY ${PROJECT_SOURCE_DIR}
        )
    endif()
endif()

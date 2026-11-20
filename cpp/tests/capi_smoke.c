/* Compiled as C: proves libsvlang_lint_model's ABI is usable without C++.
 * Loads SV-OOP-007's bad example and checks what the model says about it. */
#include <stdio.h>
#include <string.h>

#include "svlang_lint/model/CApi.h"

#define CHECK(cond)                                                       \
    do {                                                                  \
        if (!(cond)) {                                                    \
            fprintf(stderr, "capi_smoke: %s:%d: %s\n", __FILE__, __LINE__, #cond); \
            return 1;                                                     \
        }                                                                 \
    } while (0)

int main(int argc, char** argv) {
    CHECK(argc == 4); /* <testdata/common dir> <bad.sv> <facts.sv> */
    CHECK(svlang_lint_model_abi_version() == SVLANG_LINT_MODEL_ABI_VERSION);
    /* crates/sv-slang asserts the same sizes for its copies of these structs */
    if (sizeof(void*) == 8) {
        CHECK(sizeof(svlang_lint_class) == 80 && sizeof(svlang_lint_property) == 48);
        CHECK(sizeof(svlang_lint_method) == 104 && sizeof(svlang_lint_argument) == 56);
        CHECK(sizeof(svlang_lint_assignment) == 48);
        CHECK(sizeof(svlang_lint_macro) == 32 && sizeof(svlang_lint_typedef) == 32);
        CHECK(sizeof(svlang_lint_call) == 72);
        CHECK(sizeof(svlang_lint_local) == 40 && sizeof(svlang_lint_constraint) == 32);
    }

    char incdir[4096];
    snprintf(incdir, sizeof incdir, "+incdir+%s", argv[1]);
    const char* args[] = {incdir, argv[2]};
    svlang_lint_design* d = svlang_lint_design_load(2, args);
    CHECK(d != NULL);
    CHECK(svlang_lint_design_error(d) == NULL);
    CHECK(svlang_lint_design_compile_errors(d) == 0);
    CHECK(svlang_lint_class_count(d) > 10);

    uint32_t found = SVLANG_LINT_NONE;
    for (uint32_t i = 0; i < svlang_lint_class_count(d); ++i) {
        svlang_lint_class c;
        CHECK(svlang_lint_get_class(d, i, &c));
        if (strcmp(c.name, "MultiChannelMonitor") == 0) found = i;
    }
    CHECK(found != SVLANG_LINT_NONE);

    svlang_lint_class c;
    CHECK(svlang_lint_get_class(d, found, &c));
    CHECK(c.uvm == SVLANG_LINT_UVM_COMPONENT);
    CHECK(strcmp(c.base_name, "uvm_component") == 0);
    CHECK(c.base != SVLANG_LINT_NONE);
    CHECK(strstr(svlang_lint_file_name(d, c.loc.file), "bad.sv") != NULL);

    int sawStatic = 0;
    for (uint32_t i = 0; i < c.property_count; ++i) {
        svlang_lint_property p;
        CHECK(svlang_lint_get_property(d, found, i, &p));
        if (strcmp(p.name, "m_global_packet_counter") == 0) {
            sawStatic = p.is_static && !p.is_const && p.loc.line == 10;
        }
    }
    CHECK(sawStatic);

    int sawNew = 0;
    for (uint32_t i = 0; i < c.method_count; ++i) {
        svlang_lint_method m;
        CHECK(svlang_lint_get_method(d, found, i, &m));
        if (strcmp(m.name, "new") == 0) sawNew = 1;
    }
    CHECK(sawNew);

    /* Out of range and bad input are refused, not crashes, and nothing is printed. */
    CHECK(!svlang_lint_get_class(d, SVLANG_LINT_NONE, &c));
    CHECK(svlang_lint_file_name(d, SVLANG_LINT_NONE) == NULL);
    const char* json = svlang_lint_design_json(d);
    CHECK(json && strstr(json, "\"schema\":\"svlang_lint.model/1\"") && strstr(json, "MultiChannelMonitor"));
    svlang_lint_design_free(d);

    /* Arguments, locals, constraints and extern methods (ABI v3). */
    const char* factArgs[] = {incdir, argv[3]};
    d = svlang_lint_design_load(2, factArgs);
    CHECK(svlang_lint_design_error(d) == NULL && svlang_lint_design_compile_errors(d) == 0);
    found = SVLANG_LINT_NONE;
    for (uint32_t i = 0; i < svlang_lint_class_count(d); ++i) {
        CHECK(svlang_lint_get_class(d, i, &c));
        if (strcmp(c.name, "facts_item") == 0) found = i;
    }
    CHECK(found != SVLANG_LINT_NONE);
    CHECK(svlang_lint_get_class(d, found, &c));
    CHECK(c.uvm == SVLANG_LINT_UVM_SEQUENCE_ITEM);
    CHECK(c.constraint_count == 1);
    svlang_lint_constraint k;
    CHECK(svlang_lint_get_constraint(d, found, 0, &k));
    CHECK(strcmp(k.name, "addr_c") == 0 && k.loc.line == 11);
    CHECK(!svlang_lint_get_constraint(d, found, 1, &k));

    int sawSum = 0, sawProbe = 0;
    for (uint32_t i = 0; i < c.method_count; ++i) {
        svlang_lint_method m;
        CHECK(svlang_lint_get_method(d, found, i, &m));
        if (strcmp(m.name, "sum") == 0) {
            /* extern: the record points at the body after the class */
            CHECK(m.is_extern && m.declared_virtual && m.is_virtual && !m.returns_void);
            CHECK(strcmp(m.return_type, "int") == 0 && m.loc.line == 22);
            CHECK(m.argument_count == 2 && m.local_count == 1);
            svlang_lint_argument a;
            CHECK(svlang_lint_get_argument(d, found, i, 0, &a));
            CHECK(strcmp(a.name, "q") == 0 && a.direction == SVLANG_LINT_DIR_REF);
            CHECK(a.shape == SVLANG_LINT_SHAPE_QUEUE && a.type_class == SVLANG_LINT_NONE);
            CHECK(svlang_lint_get_argument(d, found, i, 1, &a));
            CHECK(a.direction == SVLANG_LINT_DIR_IN && a.shape == SVLANG_LINT_SHAPE_OTHER);
            CHECK(!svlang_lint_get_argument(d, found, i, 2, &a));
            svlang_lint_local l;
            CHECK(svlang_lint_get_local(d, found, i, 0, &l));
            CHECK(strcmp(l.name, "total") == 0 && !l.is_virtual_interface);
            sawSum = 1;
        } else if (strcmp(m.name, "probe") == 0) {
            CHECK(m.is_extern && m.returns_void && !m.is_virtual && m.local_count == 1);
            svlang_lint_local l;
            CHECK(svlang_lint_get_local(d, found, i, 0, &l));
            CHECK(strcmp(l.name, "vif") == 0 && l.is_virtual_interface);
            /* randomize, set, and $urandom nested in set's arguments */
            CHECK(m.call_count == 3);
            svlang_lint_call k0, k1, k2;
            CHECK(svlang_lint_get_call(d, found, i, 0, &k0) && svlang_lint_get_call(d, found, i, 1, &k1) &&
                  svlang_lint_get_call(d, found, i, 2, &k2) && !svlang_lint_get_call(d, found, i, 3, &k2));
            CHECK(strcmp(k0.name, "randomize") == 0 && k0.is_system); /* built-in method */
            CHECK(strcmp(k1.name, "set") == 0 && strcmp(k1.callee_scope, "uvm_config_db") == 0);
            CHECK(k1.first_arg_is_null && k1.loc.line == 31 && strcmp(k1.receiver, "") == 0);
            CHECK(strcmp(k2.name, "$urandom") == 0 && k2.is_system && strcmp(k2.callee_scope, "") == 0);
            sawProbe = 1;
        } else if (strcmp(m.name, "new") == 0) {
            CHECK(!m.is_extern && !m.is_builtin);
        } else if (strcmp(m.name, "post_randomize") == 0) {
            svlang_lint_call k;
            CHECK(m.call_count == 1 && svlang_lint_get_call(d, found, i, 0, &k));
            CHECK(k.via_super && strcmp(k.name, "post_randomize") == 0 && strcmp(k.receiver, "super") == 0);
        } else if (strcmp(m.name, "randomize") == 0) {
            CHECK(m.is_builtin);
        }
    }
    CHECK(sawSum && sawProbe);

    /* Assignments, timing controls and super.new (ABI v5). */
    uint32_t sub = SVLANG_LINT_NONE;
    for (uint32_t i = 0; i < svlang_lint_class_count(d); ++i) {
        CHECK(svlang_lint_get_class(d, i, &c));
        if (strcmp(c.name, "facts_sub") == 0) sub = i;
    }
    CHECK(sub != SVLANG_LINT_NONE && svlang_lint_get_class(d, sub, &c));
    int sawCtor = 0, sawWrite = 0, sawWait = 0;
    for (uint32_t i = 0; i < c.method_count; ++i) {
        svlang_lint_method m;
        CHECK(svlang_lint_get_method(d, sub, i, &m));
        if (strcmp(m.name, "new") == 0) {
            CHECK(m.calls_super_new && m.super_new_index == 0 && m.super_new_loc.line == 37);
            sawCtor = 1;
        } else if (strcmp(m.name, "write") == 0) {
            svlang_lint_assignment a;
            CHECK(m.assignment_count == 2 && svlang_lint_get_assignment(d, sub, i, 0, &a));
            CHECK(strcmp(a.lhs, "t.addr") == 0 && strcmp(a.rhs, "0") == 0 && a.is_non_blocking);
            CHECK(a.root_kind == SVLANG_LINT_ROOT_ARGUMENT && strcmp(a.root_name, "t") == 0);
            CHECK(svlang_lint_get_assignment(d, sub, i, 1, &a));
            CHECK(strcmp(a.lhs, "count") == 0 && strcmp(a.rhs, "t.addr") == 0 && !a.is_non_blocking);
            CHECK(a.root_kind == SVLANG_LINT_ROOT_PROPERTY && !svlang_lint_get_assignment(d, sub, i, 2, &a));
            CHECK(!m.calls_super_new && m.super_new_index == -1);
            sawWrite = 1;
        } else if (strcmp(m.name, "wait_a_bit") == 0) {
            svlang_lint_loc t;
            CHECK(m.timing_count == 1 && svlang_lint_get_timing(d, sub, i, 0, &t) && t.line == 45);
            sawWait = 1;
        }
    }
    CHECK(sawCtor && sawWrite && sawWait);
    CHECK(strstr(svlang_lint_design_json(d), "\"extern\":true") != NULL);
    svlang_lint_design_free(d);

    const char* missing[] = {"/no/such/file.sv"};
    d = svlang_lint_design_load(1, missing);
    CHECK(svlang_lint_design_error(d) != NULL);
    CHECK(strstr(svlang_lint_design_error(d), "/no/such/file.sv") != NULL);
    CHECK(svlang_lint_class_count(d) == 0);
    svlang_lint_design_free(d);

    printf("capi_smoke: ok\n");
    return 0;
}

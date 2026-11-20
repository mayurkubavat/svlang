/* C ABI of libsvlang_lint_model: load a design, read its svlang_lint::model.
 *
 * Plain C so that Rust (crates/sv-slang), Python (ctypes) and any other tool can
 * use it with no C++ toolchain. Every string and record stays valid until
 * svlang_lint_design_free(). Ids are 0-based; SVLANG_LINT_NONE marks "no class".
 * The ABI version changes whenever a struct below changes.
 */
#ifndef SVLANG_LINT_MODEL_CAPI_H
#define SVLANG_LINT_MODEL_CAPI_H

#include <stdint.h>

#if defined(_WIN32)
#  define SVLANG_LINT_API __declspec(dllexport)
#else
#  define SVLANG_LINT_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define SVLANG_LINT_MODEL_ABI_VERSION 7u
#define SVLANG_LINT_NONE 0xFFFFFFFFu

typedef struct svlang_lint_design svlang_lint_design;

/* Values of svlang_lint_class.uvm, in svlang_lint::model::UvmKind order. */
enum {
    SVLANG_LINT_UVM_NONE = 0, SVLANG_LINT_UVM_OBJECT, SVLANG_LINT_UVM_SEQUENCE_ITEM, SVLANG_LINT_UVM_SEQUENCE,
    SVLANG_LINT_UVM_COMPONENT, SVLANG_LINT_UVM_TEST, SVLANG_LINT_UVM_ENV, SVLANG_LINT_UVM_AGENT,
    SVLANG_LINT_UVM_DRIVER, SVLANG_LINT_UVM_MONITOR, SVLANG_LINT_UVM_SEQUENCER, SVLANG_LINT_UVM_SCOREBOARD,
    SVLANG_LINT_UVM_SUBSCRIBER
};

typedef struct {
    uint32_t file, line, column;
    /* Code from a macro: file and line of the outermost expansion; 0 otherwise. */
    uint32_t exp_file, exp_line;
} svlang_lint_loc;

typedef struct {
    const char* name;
    const char* scope;
    const char* base_name;
    uint32_t base;
    uint8_t uvm;
    uint8_t is_virtual, is_parameterized, is_library;
    svlang_lint_loc loc;
    uint32_t property_count, method_count, constraint_count;
    uint32_t macro_count, typedef_count;
    uint8_t is_interface;
} svlang_lint_class;

typedef struct {
    const char* name;
    const char* type;
    uint32_t type_class;
    uint8_t visibility; /* 0 public, 1 protected, 2 local */
    uint8_t is_static, is_const, is_rand, is_virtual_interface;
    svlang_lint_loc loc;
} svlang_lint_property;

typedef struct {
    const char* name;
    uint8_t visibility;
    uint8_t is_task, is_virtual, is_pure, is_static;
    uint8_t phase; /* 0 none, 1 function phase, 2 task phase */
    uint8_t declared_virtual; /* has the `virtual` keyword itself */
    uint8_t is_builtin;       /* made by the compiler (randomize and the like) */
    uint8_t is_extern;        /* declared by a prototype; loc is the body outside the class */
    uint8_t returns_void;
    const char* return_type;
    svlang_lint_loc loc;
    uint32_t argument_count, local_count, call_count, assignment_count, timing_count;
    int32_t super_new_index;     /* top-level statement index of `super.new(...);`, or -1 */
    svlang_lint_loc super_new_loc;
    uint8_t calls_super_new;     /* super.new(...) anywhere in the body */
} svlang_lint_method;

/* Values of svlang_lint_argument.direction and .shape. */
enum { SVLANG_LINT_DIR_IN = 0, SVLANG_LINT_DIR_OUT, SVLANG_LINT_DIR_INOUT, SVLANG_LINT_DIR_REF };
enum {
    SVLANG_LINT_SHAPE_OTHER = 0, SVLANG_LINT_SHAPE_DYNAMIC_ARRAY, SVLANG_LINT_SHAPE_QUEUE,
    SVLANG_LINT_SHAPE_ASSOCIATIVE_ARRAY, SVLANG_LINT_SHAPE_FIXED_UNPACKED_ARRAY, SVLANG_LINT_SHAPE_UNPACKED_STRUCT
};

typedef struct {
    const char* name;
    const char* type;
    const char* canonical_type;
    uint32_t type_class;
    uint8_t direction, shape;
    svlang_lint_loc loc;
} svlang_lint_argument;

typedef struct {
    const char* name;
    const char* type;
    uint8_t is_virtual_interface;
    svlang_lint_loc loc;
} svlang_lint_local;

typedef struct {
    const char* name;
    svlang_lint_loc loc;
} svlang_lint_constraint;

/* A user macro used inside the class's text, by name without the backtick. */
typedef struct {
    const char* name;
    svlang_lint_loc loc;
} svlang_lint_macro;

/* A typedef declared in the class. */
typedef struct {
    const char* name;
    svlang_lint_loc loc;
} svlang_lint_typedef;

/* Values of svlang_lint_assignment.root_kind. */
enum { SVLANG_LINT_ROOT_OTHER = 0, SVLANG_LINT_ROOT_ARGUMENT, SVLANG_LINT_ROOT_LOCAL, SVLANG_LINT_ROOT_PROPERTY };

/* An assignment in a method body, in walk order. */
typedef struct {
    const char* lhs;       /* left side as written */
    const char* rhs;       /* right side as written */
    const char* root_name; /* argument, local or property the left side is rooted in */
    svlang_lint_loc loc;
    uint8_t is_non_blocking, root_kind;
} svlang_lint_assignment;

/* A call in a method body, in walk order (nested calls included). */
typedef struct {
    const char* name;         /* as written: "randomize", "$urandom" */
    const char* callee_scope; /* class (generic name) or package declaring the callee; "" if none */
    const char* receiver;     /* object called on, as written ("m_env.ap"); "" if none */
    svlang_lint_loc loc;
    svlang_lint_loc first_arg_loc;
    uint8_t is_system, callee_declared_virtual, callee_builtin, first_arg_is_null, via_super;
} svlang_lint_call;

SVLANG_LINT_API uint32_t svlang_lint_model_abi_version(void);

/* argv: compile options as given to svlang-lint, without the program name.
 * Never returns NULL; check svlang_lint_design_error(). */
SVLANG_LINT_API svlang_lint_design* svlang_lint_design_load(int argc, const char* const* argv);
SVLANG_LINT_API void svlang_lint_design_free(svlang_lint_design* d);
/* NULL when the design loaded (compile errors in the code do not count). */
SVLANG_LINT_API const char* svlang_lint_design_error(const svlang_lint_design* d);
SVLANG_LINT_API uint32_t svlang_lint_design_compile_errors(const svlang_lint_design* d);
/* stage: 0 parse, 1 elaborate, 2 model build. Milliseconds. */
SVLANG_LINT_API double svlang_lint_design_time_ms(const svlang_lint_design* d, int stage);

SVLANG_LINT_API uint32_t svlang_lint_file_count(const svlang_lint_design* d);
SVLANG_LINT_API const char* svlang_lint_file_name(const svlang_lint_design* d, uint32_t file);

SVLANG_LINT_API uint32_t svlang_lint_class_count(const svlang_lint_design* d);
/* Return 1 and fill *out, or 0 when an id is out of range. */
SVLANG_LINT_API int svlang_lint_get_class(const svlang_lint_design* d, uint32_t cls, svlang_lint_class* out);
SVLANG_LINT_API int svlang_lint_get_property(const svlang_lint_design* d, uint32_t cls, uint32_t index,
                                       svlang_lint_property* out);
SVLANG_LINT_API int svlang_lint_get_method(const svlang_lint_design* d, uint32_t cls, uint32_t index,
                                     svlang_lint_method* out);
SVLANG_LINT_API int svlang_lint_get_argument(const svlang_lint_design* d, uint32_t cls, uint32_t method,
                                       uint32_t index, svlang_lint_argument* out);
SVLANG_LINT_API int svlang_lint_get_local(const svlang_lint_design* d, uint32_t cls, uint32_t method,
                                    uint32_t index, svlang_lint_local* out);
SVLANG_LINT_API int svlang_lint_get_call(const svlang_lint_design* d, uint32_t cls, uint32_t method,
                                   uint32_t index, svlang_lint_call* out);
SVLANG_LINT_API int svlang_lint_get_assignment(const svlang_lint_design* d, uint32_t cls, uint32_t method,
                                         uint32_t index, svlang_lint_assignment* out);
/* Delays, event controls and waits in a method body, in walk order. */
SVLANG_LINT_API int svlang_lint_get_timing(const svlang_lint_design* d, uint32_t cls, uint32_t method,
                                     uint32_t index, svlang_lint_loc* out);
SVLANG_LINT_API int svlang_lint_get_constraint(const svlang_lint_design* d, uint32_t cls, uint32_t index,
                                         svlang_lint_constraint* out);
SVLANG_LINT_API int svlang_lint_get_macro(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_macro* out);
SVLANG_LINT_API int svlang_lint_get_typedef(const svlang_lint_design* d, uint32_t cls, uint32_t index,
                                      svlang_lint_typedef* out);

/* The whole model as JSON (schema svlang_lint.model/1), made on first call. */
SVLANG_LINT_API const char* svlang_lint_design_json(svlang_lint_design* d);

#ifdef __cplusplus
}
#endif

#endif

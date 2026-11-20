//! Raw bindings to `cpp/include/svlang-lint/model/CApi.h`. Keep this file in step
//! with that header; `dv-model` is the safe API tools should use.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};

/// The ABI version this binding was written for; `dv-model` refuses a library
/// that reports another one (`svlang_lint_model_abi_version`).
pub const ABI_VERSION: u32 = 7;
/// `SVLANG_LINT_NONE`: a class id meaning "no class".
pub const NONE: u32 = u32::MAX;

/// Opaque handle to a loaded design; free it with [`svlang_lint_design_free`].
#[repr(C)]
pub struct svlang_lint_design {
    _private: [u8; 0],
}

/// A source position (`svlang_lint_loc`). Lines and columns start at 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct svlang_lint_loc {
    /// Index into the design's file table; 0 is "no file".
    pub file: u32,
    /// 1-based line; 0 when unknown.
    pub line: u32,
    /// 1-based byte column.
    pub column: u32,
    /// For code from a macro: file of the outermost expansion.
    pub exp_file: u32,
    /// For code from a macro: line of the outermost expansion; 0 otherwise.
    pub exp_line: u32,
}

/// A class (`svlang_lint_class`); see `dv_model::Class`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_class {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Enclosing package or class; empty at `$unit`.
    pub scope: *const c_char,
    /// Base class as written; empty when none.
    pub base_name: *const c_char,
    /// Base class id, or [`NONE`].
    pub base: u32,
    /// `UvmKind` as a number, in `dv_model::UvmKind` order.
    pub uvm: u8,
    /// Nonzero for a `virtual class`.
    pub is_virtual: u8,
    /// Nonzero for a parameterized class.
    pub is_parameterized: u8,
    /// Nonzero for a class from a library path.
    pub is_library: u8,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
    /// Number of properties ([`svlang_lint_get_property`]).
    pub property_count: u32,
    /// Number of methods ([`svlang_lint_get_method`]).
    pub method_count: u32,
    /// Number of constraint blocks ([`svlang_lint_get_constraint`]).
    pub constraint_count: u32,
    /// Number of macro uses ([`svlang_lint_get_macro`]).
    pub macro_count: u32,
    /// Number of typedefs ([`svlang_lint_get_typedef`]).
    pub typedef_count: u32,
    /// Nonzero for an `interface class`.
    pub is_interface: u8,
}

/// A class property (`svlang_lint_property`); see `dv_model::Property`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_property {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Type as printed by slang.
    pub type_: *const c_char,
    /// The type's class id, or [`NONE`].
    pub type_class: u32,
    /// 0 public, 1 protected, 2 local.
    pub visibility: u8,
    /// Nonzero when `static`.
    pub is_static: u8,
    /// Nonzero when `const`.
    pub is_const: u8,
    /// Nonzero when `rand` or `randc`.
    pub is_rand: u8,
    /// Nonzero for a virtual interface or an array of them.
    pub is_virtual_interface: u8,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

/// A method (`svlang_lint_method`); see `dv_model::Method`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_method {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// 0 public, 1 protected, 2 local.
    pub visibility: u8,
    /// Nonzero for a task.
    pub is_task: u8,
    /// Nonzero for a virtual method (declared or by override).
    pub is_virtual: u8,
    /// Nonzero for `pure virtual`.
    pub is_pure: u8,
    /// Nonzero when `static`.
    pub is_static: u8,
    /// 0 not a phase method, 1 function phase, 2 task phase.
    pub phase: u8,
    /// Nonzero when the `virtual` keyword is written.
    pub declared_virtual: u8,
    /// Nonzero for a compiler-made method.
    pub is_builtin: u8,
    /// Nonzero when declared by a prototype in the class.
    pub is_extern: u8,
    /// Nonzero for tasks and void functions.
    pub returns_void: u8,
    /// Return type as written.
    pub return_type: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
    /// Number of arguments ([`svlang_lint_get_argument`]).
    pub argument_count: u32,
    /// Number of locals ([`svlang_lint_get_local`]).
    pub local_count: u32,
    /// Number of calls ([`svlang_lint_get_call`]).
    pub call_count: u32,
    /// Number of assignments ([`svlang_lint_get_assignment`]).
    pub assignment_count: u32,
    /// Number of timing controls ([`svlang_lint_get_timing`]).
    pub timing_count: u32,
    /// Top-level statement index of `super.new(...);`, or -1.
    pub super_new_index: i32,
    /// Where that `super.new(...);` is.
    pub super_new_loc: svlang_lint_loc,
    /// Nonzero when `super.new` is called anywhere in the body.
    pub calls_super_new: u8,
}

/// An assignment in a method body (`svlang_lint_assignment`); see `dv_model::Assignment`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_assignment {
    /// Left side as written.
    pub lhs: *const c_char,
    /// Right side as written.
    pub rhs: *const c_char,
    /// The argument, local or property assigned into.
    pub root_name: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
    /// Nonzero for `<=`.
    pub is_non_blocking: u8,
    /// 0 other, 1 argument, 2 local, 3 property.
    pub root_kind: u8,
}

/// A method argument (`svlang_lint_argument`); see `dv_model::Argument`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_argument {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Type as printed by slang.
    pub type_: *const c_char,
    /// Type with typedefs resolved.
    pub canonical_type: *const c_char,
    /// The type's class id, or [`NONE`].
    pub type_class: u32,
    /// 0 input, 1 output, 2 inout, 3 ref.
    pub direction: u8,
    /// `TypeShape` as a number, in `dv_model::TypeShape` order.
    pub shape: u8,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

/// A local variable of a method (`svlang_lint_local`); see `dv_model::Local`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_local {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Type as printed by slang.
    pub type_: *const c_char,
    /// Nonzero for a virtual interface or an array of them.
    pub is_virtual_interface: u8,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

/// A call in a method body (`svlang_lint_call`); see `dv_model::Call`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_call {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Class or package declaring the callee; empty if none.
    pub callee_scope: *const c_char,
    /// Object called on, as written; empty if none.
    pub receiver: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
    /// Where the first argument starts.
    pub first_arg_loc: svlang_lint_loc,
    /// Nonzero for a system or built-in call.
    pub is_system: u8,
    /// Nonzero when the callee has the `virtual` keyword.
    pub callee_declared_virtual: u8,
    /// Nonzero when the callee is compiler-made.
    pub callee_builtin: u8,
    /// Nonzero when the first argument is `null`.
    pub first_arg_is_null: u8,
    /// Nonzero for `super.name(...)`.
    pub via_super: u8,
}

/// A constraint block (`svlang_lint_constraint`); see `dv_model::Constraint`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_constraint {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

/// A user macro used in a class (`svlang_lint_macro`); see `dv_model::MacroUse`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_macro {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

/// A typedef in a class (`svlang_lint_typedef`); see `dv_model::Typedef`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct svlang_lint_typedef {
    /// Name, NUL-terminated; valid until the design is freed.
    pub name: *const c_char,
    /// Where it is declared or written.
    pub loc: svlang_lint_loc,
}

extern "C" {
    /// The library's ABI version; must equal [`ABI_VERSION`].
    pub fn svlang_lint_model_abi_version() -> u32;
    /// Loads a design from compile options as given to svlang-lint (no program
    /// name). Never returns null; check [`svlang_lint_design_error`].
    pub fn svlang_lint_design_load(argc: c_int, argv: *const *const c_char) -> *mut svlang_lint_design;
    /// Frees a design and every string and record read from it.
    pub fn svlang_lint_design_free(d: *mut svlang_lint_design);
    /// Null when the design loaded (compile errors in the code do not count),
    /// else why it did not.
    pub fn svlang_lint_design_error(d: *const svlang_lint_design) -> *const c_char;
    /// Number of compile errors slang reported.
    pub fn svlang_lint_design_compile_errors(d: *const svlang_lint_design) -> u32;
    /// Milliseconds spent in a stage: 0 parse, 1 elaborate, 2 model build.
    pub fn svlang_lint_design_time_ms(d: *const svlang_lint_design, stage: c_int) -> f64;
    /// Size of the file table (entry 0 is "no file").
    pub fn svlang_lint_file_count(d: *const svlang_lint_design) -> u32;
    /// Name of file `file`.
    pub fn svlang_lint_file_name(d: *const svlang_lint_design, file: u32) -> *const c_char;
    /// Number of classes; ids run from 0.
    pub fn svlang_lint_class_count(d: *const svlang_lint_design) -> u32;
    /// Fills `*out` with class `cls` and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_class(d: *const svlang_lint_design, cls: u32, out: *mut svlang_lint_class) -> c_int;
    /// Fills `*out` with property `index` (of class `cls`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_property(
        d: *const svlang_lint_design,
        cls: u32,
        index: u32,
        out: *mut svlang_lint_property,
    ) -> c_int;
    /// Fills `*out` with method `index` (of class `cls`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_method(
        d: *const svlang_lint_design,
        cls: u32,
        index: u32,
        out: *mut svlang_lint_method,
    ) -> c_int;
    /// Fills `*out` with argument `index` (of class `cls`, method `method`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_argument(
        d: *const svlang_lint_design,
        cls: u32,
        method: u32,
        index: u32,
        out: *mut svlang_lint_argument,
    ) -> c_int;
    /// Fills `*out` with local `index` (of class `cls`, method `method`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_local(
        d: *const svlang_lint_design,
        cls: u32,
        method: u32,
        index: u32,
        out: *mut svlang_lint_local,
    ) -> c_int;
    /// Fills `*out` with call `index` (of class `cls`, method `method`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_call(
        d: *const svlang_lint_design,
        cls: u32,
        method: u32,
        index: u32,
        out: *mut svlang_lint_call,
    ) -> c_int;
    /// Fills `*out` with assignment `index` (of class `cls`, method `method`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_assignment(
        d: *const svlang_lint_design,
        cls: u32,
        method: u32,
        index: u32,
        out: *mut svlang_lint_assignment,
    ) -> c_int;
    /// Fills `*out` with the location of timing control `index` (of class
    /// `cls`, method `method`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_timing(
        d: *const svlang_lint_design,
        cls: u32,
        method: u32,
        index: u32,
        out: *mut svlang_lint_loc,
    ) -> c_int;
    /// Fills `*out` with constraint block `index` (of class `cls`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_constraint(
        d: *const svlang_lint_design,
        cls: u32,
        index: u32,
        out: *mut svlang_lint_constraint,
    ) -> c_int;
    /// Fills `*out` with macro use `index` (of class `cls`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_macro(
        d: *const svlang_lint_design,
        cls: u32,
        index: u32,
        out: *mut svlang_lint_macro,
    ) -> c_int;
    /// Fills `*out` with typedef `index` (of class `cls`) and returns 1,
    /// or returns 0 when an id is out of range.
    pub fn svlang_lint_get_typedef(
        d: *const svlang_lint_design,
        cls: u32,
        index: u32,
        out: *mut svlang_lint_typedef,
    ) -> c_int;
    /// The whole model as JSON (schema `svlang_lint.model/1`), made on first call.
    pub fn svlang_lint_design_json(d: *mut svlang_lint_design) -> *const c_char;
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_matches_the_c_header() {
        use std::mem::size_of;
        // Pointer-sized fields first, then fixed-width ones, as in CApi.h.
        assert_eq!(size_of::<super::svlang_lint_loc>(), 20);
        // 3 pointers, base, 4 flags, loc, 5 counts, is_interface, padded to 8.
        assert_eq!(size_of::<super::svlang_lint_class>(), 80);
        assert_eq!(size_of::<super::svlang_lint_macro>(), 32);
        assert_eq!(size_of::<super::svlang_lint_typedef>(), 32);
        // Sizes with the padding a C compiler adds (8-byte pointers).
        assert_eq!(size_of::<super::svlang_lint_property>(), 48);
        assert_eq!(size_of::<super::svlang_lint_method>(), 104);
        assert_eq!(size_of::<super::svlang_lint_assignment>(), 48);
        assert_eq!(size_of::<super::svlang_lint_argument>(), 56);
        assert_eq!(size_of::<super::svlang_lint_local>(), 40);
        assert_eq!(size_of::<super::svlang_lint_constraint>(), 32);
        assert_eq!(size_of::<super::svlang_lint_call>(), 72);
        assert_eq!(unsafe { super::svlang_lint_model_abi_version() }, super::ABI_VERSION);
    }
}

//! The style rules: the Rust twins of `cpp/src/rules/style/`, one file per rule,
//! tested by `testdata/rules/<area>/<ID>/`.

use crate::profile::*;
use crate::{Check, Rule, Severity};

mod class_member_ordering;
mod class_member_prefix;
pub(crate) mod configurable_naming_engine;
mod configurable_style_engine;
mod constraint_name_suffix;
mod duplicate_macro_redefinition;
mod explicit_literal_width;
mod extern_method_threshold;
mod include_guard;
mod inconsistent_block_indentation;
mod interface_naming_convention;
mod macro_naming_convention;
mod macro_trailing_semicolon;
mod mandatory_matching_end_label;
mod max_line_length;
mod method_banner_style;
mod mismatched_end_label;
mod multiple_declarations_per_line;
mod one_statement_per_line;
mod package_naming_convention;
mod port_and_param_column_alignment;
mod section_header_banner;
mod space_keyword_paren;
mod todo_comment_tracker;
mod trailing_whitespace;
mod typedef_name_suffix;
mod unclosed_block_waiver;
mod unparenthesized_macro_arg;
mod unused_include_directive;
mod uvm_class_type_suffix;
mod uvm_component_naming_convention;
mod uvm_create_string_mismatch;
mod uvm_tlm_port_suffix;

/// Every rule of this category.
pub(crate) fn rules() -> Vec<Rule> {
    vec![
        Rule { id: "UVM-NAM-001", name: "UvmComponentNamingConvention", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces standard naming conventions for UVM components: classes derived from uvm_driver must end with '_driver', uvm_monitor with '_monitor', uvm_agent with '_agent', uvm_env with '_env', and uvm_test with '_test'.", check: Check::Model(uvm_component_naming_convention::uvm_component_naming_convention),
        },
        Rule { id: "SVL-NAM-001", name: "PackageNamingConvention", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces that package identifiers end with '_pkg' (e.g., 'axi_pkg') to standardize namespaces.", check: Check::Syntax(package_naming_convention::package_naming),
        },
        Rule { id: "SVL-NAM-002", name: "InterfaceNamingConvention", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces that interface identifiers end with '_if' (e.g., 'axi_if') to standardize interface definitions.", check: Check::Syntax(interface_naming_convention::interface_naming),
        },
        Rule { id: "UVM-NAM-002", name: "UvmClassTypeSuffix", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces standard verification naming suffix conventions for classes extending UVM base components (e.g. _seq, _item, _driver, _monitor, _scoreboard, _env).", check: Check::Syntax(uvm_class_type_suffix::uvm_class_suffix),
        },
        Rule { id: "SVL-NAM-100", name: "ConfigurableNamingEngine", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Parameterizable naming convention engine validating regex, prefix, suffix, and casing conventions across packages, interfaces, UVM classes, covergroups, and constraints.", check: Check::Mixed(configurable_naming_engine::naming_engine),
        },
        Rule { id: "UVM-NAM-003", name: "ClassMemberPrefix", category: "Naming", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces that member properties of uvm_component classes begin with 'm_' prefix to distinguish local member state from arguments and parameters.", check: Check::Model(class_member_prefix::class_member_prefix),
        },
        Rule { id: "SVL-STY-001", name: "MacroTrailingSemicolon", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Flags unnecessary semicolons following `uvm_* macro invocations that create empty statements in declarative or procedural scopes.", check: Check::Syntax(macro_trailing_semicolon::macro_trailing_semicolon),
        },
        Rule { id: "SVL-STY-002", name: "TodoCommentTracker", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Tracks unresolved TODO, FIXME, ICEBOX, HACK, and XXX comment markers across the codebase. Ensures temporary developer placeholders are addressed before tape-out or milestone delivery.", check: Check::Syntax(todo_comment_tracker::todo_comments),
        },
        Rule { id: "SVL-NAM-003", name: "TypedefNameSuffix", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces naming conventions for typedefs: enums must end with '_e', and structs/data types must end with '_t'.", check: Check::Syntax(typedef_name_suffix::typedef_name_suffix),
        },
        Rule { id: "SVL-STY-003", name: "MandatoryMatchingEndLabel", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces explicit and matching end labels on functions, tasks, and classes (e.g., endfunction : name). Prevents scope delimiter ambiguity in large verification codebases.", check: Check::Syntax(mandatory_matching_end_label::missing_end_label),
        },
        Rule { id: "SVL-STY-004", name: "MismatchedEndLabel", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Flags mismatched end labels on functions, tasks, and classes (e.g., function void foo(); ... endfunction : bar). Prevents scope delimiter confusion and copy-paste errors.", check: Check::Syntax(mismatched_end_label::mismatched_end_label),
        },
        Rule { id: "SVL-STY-005", name: "MethodBannerStyle", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Standardizes method separator pre-banners. Enforces 70–80 character uniform dashed separator lines and flags ragged or arbitrary-width comment separators.", check: Check::Syntax(method_banner_style::method_banner) },
        Rule { id: "SVL-STY-006", name: "SectionHeaderBanner", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Standardizes major section divider banners to uniform 70–80 character widths. Prevents ragged ASCII banners across verification packages and testbench files.", check: Check::Syntax(section_header_banner::section_banner),
        },
        Rule { id: "SVL-STY-007", name: "ExternMethodThreshold", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &["MG-9.1"], experimental: false, native: false, about: "Enforces 'extern' declaration for class methods exceeding N lines of code (default: 5 lines). Keeps class declarations concise and readable as clean interface definitions.", check: Check::Syntax(extern_method_threshold::extern_method_threshold),
        },
        Rule { id: "SVL-STY-008", name: "ExplicitLiteralWidth", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Disallow unsized integer and numeric literals without explicit bit-width declarations. Prevents silent 32-bit zero-extension and sign extension bugs.", check: Check::Syntax(explicit_literal_width::explicit_literal_width),
        },
        Rule { id: "SVL-STY-009", name: "OneStatementPerLine", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Prohibit multiple procedural statements on the same line separated by semicolons. Enforces clean formatting and deterministic single-stepping in simulation debuggers.", check: Check::Syntax(one_statement_per_line::one_statement_per_line),
        },
        Rule { id: "SVL-STY-010", name: "SpaceKeywordParen", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Requires a single space between control keywords (if, for, foreach, while, case) and opening parenthesis. Enforces common SystemVerilog code formatting guidelines.", check: Check::Syntax(space_keyword_paren::space_keyword_paren),
        },
        Rule { id: "SVL-STY-011", name: "IncludeGuard", category: "Style", severity: Severity::Error, profiles: STYLE, guidelines: &["MG-1.3"], experimental: false, native: false, about: "Every included header file (.svh) must be wrapped in standard macro include guards (`ifndef / `define / `endif) to prevent circular definitions and compiler redefinition errors.", check: Check::Syntax(include_guard::include_guard) },
        Rule { id: "SVL-STY-012", name: "TrailingWhitespace", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Disallows trailing whitespace (spaces and tabs) at the end of lines in source files.", check: Check::Syntax(trailing_whitespace::trailing_whitespace),
        },
        Rule { id: "SVL-STY-013", name: "MaxLineLength", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Flags source lines exceeding 120 characters (option: max_columns) to preserve readability across split-view editors and terminal reviews.", check: Check::Syntax(max_line_length::max_line_length) },
        Rule { id: "SVL-STY-100", name: "ConfigurableStyleEngine", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Parameterizable code style engine enforcing tab prohibitions, trailing whitespace limits, maximum line lengths, matching block end labels, and file naming.", check: Check::Syntax(configurable_style_engine::style_engine),
        },
        Rule { id: "UVM-NAM-004", name: "UvmCreateStringMismatch", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "String argument passed to type_id::create() must match the receiving variable name. Mismatched instance names break uvm_config_db hierarchy paths and targeted overrides.", check: Check::Syntax(uvm_create_string_mismatch::create_string_mismatch),
        },
        Rule { id: "SVL-NAM-005", name: "ConstraintNameSuffix", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces a naming standard requiring all constraint blocks to end with '_c' suffix.", check: Check::Model(constraint_name_suffix::constraint_name_suffix),
        },
        Rule { id: "UVM-NAM-005", name: "UvmTlmPortSuffix", category: "Naming", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Enforces standard UVM TLM port suffix naming conventions (_ap for analysis ports, _imp for imps, _export for exports, _port for ports, _fifo for TLM FIFOs).", check: Check::Syntax(uvm_tlm_port_suffix::tlm_port_suffix),
        },
        Rule { id: "SVL-WVR-001", name: "UnclosedBlockWaiver", category: "Maintainability", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: true, about: "Flags scoped block waiver comments (svlang-lint:off, verissimo_lint: waive_start) that are never closed with a matching on/waive_stop directive before the end of the file.", check: Check::Syntax(unclosed_block_waiver::unclosed_block_waiver) },
        Rule { id: "SVL-STY-014", name: "InconsistentBlockIndentation", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Flags multi-line begin/end, class, module, function, and task blocks whose direct child items have unindented or inconsistent column alignment.", check: Check::Syntax(inconsistent_block_indentation::inconsistent_block_indentation) },
        Rule { id: "SVL-STY-015", name: "MultipleDeclarationsPerLine", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Flags multiple variable or property declarations placed on a single source line inside multi-line blocks, classes, or modules.", check: Check::Syntax(multiple_declarations_per_line::multiple_declarations_per_line) },
        Rule { id: "SVL-STY-016", name: "UnparenthesizedMacroArg", category: "Style", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Flags function-like expression macros whose formal parameters appear adjacent to operators in the macro body without enclosing parentheses.", check: Check::Syntax(unparenthesized_macro_arg::unparenthesized_macro_arg) },
        Rule { id: "SVL-STY-017", name: "UnusedIncludeDirective", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Flags top-level `include directives outside package aggregation bodies whose included headers contribute no tokens or used macros.", check: Check::Syntax(unused_include_directive::unused_include_directive) },
        Rule { id: "SVL-STY-018", name: "ClassMemberOrdering", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Enforces canonical class member ordering: typedefs and parameters, properties, constraints and covergroups, constructor new(), then methods.", check: Check::Syntax(class_member_ordering::class_member_ordering) },
        Rule { id: "SVL-STY-019", name: "PortAndParamColumnAlignment", category: "Style", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Flags port and parameter identifiers in tabular-aligned multi-line module or interface headers that drift out of column alignment.", check: Check::Syntax(port_and_param_column_alignment::port_and_param_column_alignment) },
        Rule { id: "SVL-NAM-004", name: "MacroNamingConvention", category: "Naming", severity: Severity::Info, profiles: STYLE, guidelines: &[], experimental: false, native: true, about: "Enforces UPPER_SNAKE_CASE naming on user-defined preprocessor macros (option: allow_prefix), exempting standard UVM and compiler macros.", check: Check::Syntax(macro_naming_convention::macro_naming_convention) },
        Rule { id: "SVL-MNT-056", name: "DuplicateMacroRedefinition", category: "Maintainability", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: true, about: "Flags redefining an already-defined macro in the same file with a different replacement body without an intervening `undef or mutually exclusive conditional branch.", check: Check::Syntax(duplicate_macro_redefinition::duplicate_macro_redefinition) },
    ]
}

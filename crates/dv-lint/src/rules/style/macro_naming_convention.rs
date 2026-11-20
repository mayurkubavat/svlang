//! SV-STY-026 (MacroNamingConvention): Uses the Preprocessor Event Table
//! ([`svlang::syntax::Tree::macro_defs`]) to enforce `UPPER_SNAKE_CASE` naming
//! on user-defined `` `define `` macros (option: `allow_prefix`), exempting
//! standard UVM (`uvm_*`) and double-underscore system/compiler macros.

use crate::support::source_layout::at;
use crate::support::trivia::library_file;
use crate::syntax::Cst;
use crate::{Severity, Sink};

fn is_upper_snake(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_uppercase() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

pub(crate) fn macro_naming_convention(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let allow_prefix = crate::options::get_str("SV-STY-026", "allow_prefix", "");
    for def in layout.main_macro_defs(c.tree) {
        let name = &*def.name;
        if library_file(c, def.origin.file)
            || name.starts_with("uvm_")
            || name.starts_with("UVM_")
            || name.starts_with("__")
            || (!allow_prefix.is_empty() && name.starts_with(allow_prefix.as_str()))
        {
            continue;
        }
        if !is_upper_snake(name) {
            out.add(
                "SV-STY-026",
                "MacroNamingConvention",
                Severity::Info,
                format!(
                    "Macro '`{name}' does not follow UPPER_SNAKE_CASE naming convention. Use uppercase letters, digits, and underscores for preprocessor macros to distinguish them from variables and functions."
                ),
                at(c, def.origin.file, def.origin.offset as usize),
            );
        }
    }
}

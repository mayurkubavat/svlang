//! SVL-OOP-016 (MissingSuperInDoHook): Flags `do_copy`, `do_compare`,
//! `do_print`, `do_pack`, and `do_unpack` overrides in derived user classes
//! whose base class is also a user class (or explicitly overrides the same
//! hook) when the override omits `super.do_*()`, silently dropping base-class
//! fields from UVM object operations.

use svlang_model::Design;

use crate::find_method;
use crate::support::uvm_classes::user_classes;
use crate::{Severity, Sink};

const DO_HOOKS: [&str; 5] = ["do_copy", "do_compare", "do_print", "do_pack", "do_unpack"];

pub(crate) fn missing_super_in_do_hook(d: &Design, out: &mut Sink) {
    for cls in user_classes(d) {
        if cls.is_parameterized || cls.loc.expansion.is_some() {
            continue;
        }
        let Some(base_id) = cls.base else { continue };
        let base = d.class(base_id);
        if base.name.starts_with("uvm_") {
            continue;
        }
        let base_has_state_or_hook = |hook: &str| {
            !base.properties.is_empty()
                || find_method(base, hook).is_some()
                || d.ancestors(base_id)
                    .take_while(|a| !a.name.starts_with("uvm_"))
                    .any(|a| !a.properties.is_empty() || find_method(a, hook).is_some())
        };
        for m in &cls.methods {
            if m.is_extern || m.is_pure || m.loc.expansion.is_some() {
                continue;
            }
            if !DO_HOOKS.contains(&m.name.as_str()) || !base_has_state_or_hook(&m.name) {
                continue;
            }
            let calls_super = m.calls.iter().any(|k| k.name == m.name && k.receiver == "super");
            if !calls_super {
                out.add(
                    "SVL-OOP-016",
                    "MissingSuperInDoHook",
                    Severity::Warning,
                    format!(
                        "Method '{}::{}' overrides UVM hook in derived class extending '{}' without calling 'super.{}(...)'. Base-class fields in '{}' will be omitted from {}.",
                        cls.name,
                        m.name,
                        base.name,
                        m.name,
                        base.name,
                        m.name.trim_start_matches("do_")
                    ),
                    m.loc,
                );
            }
        }
    }
}

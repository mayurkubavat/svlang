//! UVM-PHS-012 (MismatchedSuperPhaseCall): Overridden UVM phase method calls
//! `super.<wrong_phase>(phase)` instead of `super.<same_phase>(phase)`.

use svlang_model::Design;

use crate::support::uvm_kinds::is_component;
use crate::{is_function_phase, is_task_phase, Severity, Sink};

pub(crate) fn mismatched_super_phase_call(d: &Design, out: &mut Sink) {
    for cls in d.classes() {
        if cls.is_library || !is_component(d, cls) {
            continue;
        }
        for sub in &cls.methods {
            if sub.is_builtin || sub.is_extern || !(is_function_phase(&sub.name) || is_task_phase(&sub.name)) {
                continue;
            }
            for call in &sub.calls {
                if !call.via_super {
                    continue;
                }
                if (is_function_phase(&call.name) || is_task_phase(&call.name)) && call.name != sub.name {
                    out.add(
                        "UVM-PHS-012",
                        "MismatchedSuperPhaseCall",
                        Severity::Warning,
                        format!(
                            "Phase method '{}' in component '{}' calls 'super.{}(...)' instead of 'super.{}(phase)'. \
Calling a different base phase executes phase callbacks out of order and skips '{}' base initialization.",
                            sub.name, cls.name, call.name, sub.name, sub.name
                        ),
                        call.loc,
                    );
                }
            }
        }
    }
}

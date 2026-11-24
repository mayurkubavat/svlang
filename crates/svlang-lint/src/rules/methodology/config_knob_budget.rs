//! SVL-MNT-037 (ConfigKnobBudget): Flags a configuration object (a uvm_object
//! named *cfg* or *config*) that declares more than max_knobs (default 12) data
//! members of its own.

use crate::support::class_roles::{role, user_classes, Role};
use crate::{Severity, Sink};
use svlang_model::Design;

pub(crate) fn config_knob_budget(d: &Design, out: &mut Sink) {
    // Option `max_knobs` (rules.SVL-MNT-037.options).
    fn limit() -> usize {
        crate::options::get("SVL-MNT-037", "max_knobs", 12) as usize
    }
    for cls in user_classes(d) {
        if role(d, cls) != Role::Config {
            continue;
        }
        let knobs = cls.properties.iter().filter(|p| !p.is_static && !p.is_const).count();
        if knobs <= limit() {
            continue;
        }
        out.add(
            "SVL-MNT-037",
            "ConfigKnobBudget",
            Severity::Warning,
            format!(
                "Config '{}' has {knobs} knobs (budget {}). Group related knobs into sub-config objects so each agent or feature \
reads only its own.",
                cls.name,
                limit()
            ),
            cls.loc,
        );
    }
}

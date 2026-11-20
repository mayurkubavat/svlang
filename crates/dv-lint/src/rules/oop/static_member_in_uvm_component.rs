//! SV-OOP-007 (StaticMemberInUvmComponent): Non-const static variable declared
//! in a uvm_component subclass is shared across all instances simulation-wide.

use crate::{Severity, Sink};
use dv_model::{Design, Visibility};

pub(crate) fn static_member_in_uvm_component(d: &Design, out: &mut Sink) {
    for cls in d.classes() {
        if cls.is_parameterized || !cls.uvm.is_component() || cls.name == "uvm_component" {
            continue;
        }
        // Dedicated singleton services restrict direct construction (`local`/`protected` new()).
        if cls.methods.iter().any(|m| !m.is_builtin && m.name == "new" && m.visibility != Visibility::Public) {
            continue;
        }
        for p in &cls.properties {
            if !p.is_static || p.is_const {
                continue;
            }
            out.add(
                "SV-OOP-007",
                "StaticMemberInUvmComponent",
                Severity::Error,
                format!(
                    "Non-const static member property '{}' in uvm_component class '{}'. In SystemVerilog, static class members are shared across all instances \
simulation-wide. In multi-channel environments (e.g. multiple agents), instances will overwrite each other's state \
causing race conditions. Use an instance-scoped member or a dedicated singleton service instead.",
                    p.name, cls.name
                ),
                p.loc,
            );
        }
    }
}

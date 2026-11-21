//! SV-OOP-017 (TransitiveVirtualCallInNew): Constructor `new()` calls a
//! non-virtual helper method on `this` that transitively invokes a virtual method.

use crate::support::call_graph;
use crate::{Severity, Sink};
use svlang_model::Design;

pub(crate) fn transitive_virtual_call_in_new(d: &Design, out: &mut Sink) {
    for hit in call_graph::ctor_transitive_virtual_calls(d) {
        out.add(
            "SV-OOP-017",
            "TransitiveVirtualCallInNew",
            Severity::Warning,
            format!(
                "Constructor 'new' of class '{}' calls helper '{}' which transitively invokes virtual method '{}'. \
During base construction, derived class properties are still uninitialized when overridden virtual methods execute.",
                hit.class_name, hit.helper_name, hit.virtual_name
            ),
            hit.call_loc,
        );
    }
}

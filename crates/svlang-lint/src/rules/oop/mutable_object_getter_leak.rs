//! SVL-OOP-017 (MutableObjectGetterLeak): Flags getter functions (`get_*()`)
//! returning direct handles to mutable internal `uvm_object` / data-class
//! properties (excluding primitives, enums, structs, queues/arrays, and
//! non-cloneable `uvm_component` / TLM / RAL handles).

use crate::support::class_items::{class_decls, declarators, own};
use crate::support::data_items::{data_items, methods, routine_name};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

const VALUE_KEYWORDS: &[&str] = &[
    "int",
    "integer",
    "bit",
    "logic",
    "reg",
    "byte",
    "shortint",
    "longint",
    "time",
    "real",
    "shortreal",
    "realtime",
    "string",
    "void",
    "chandle",
    "event",
    "enum",
    "struct",
    "union",
    "virtual",
    "type",
];

fn is_cloneable_object_type(c: &Cst, dt: NodeId) -> bool {
    let Some(first) = c.first_tok(dt).map(|t| c.text_of(t)) else {
        return false;
    };
    if VALUE_KEYWORDS.contains(&first) {
        return false;
    }
    let raw = c.tree.bare_text(dt);
    let base = raw.split('#').next().unwrap_or(&raw).rsplit("::").next().unwrap_or(&raw).trim();
    if base.is_empty() || VALUE_KEYWORDS.contains(&base) {
        return false;
    }
    let low = base.to_ascii_lowercase();
    // Value/enum/struct typedef conventions (`addr_t`, `op_e`, `pkt_s`) are copied by value.
    if ["_t", "_e", "_enum", "_s", "_u", "_struct", "_union"].iter().any(|sfx| low.ends_with(sfx)) {
        return false;
    }
    // UVM components, TLM ports, phases, and RAL blocks/registers are structural singletons/hierarchy
    // nodes (`uvm_component::clone()` is illegal in IEEE 1800.2 §5.3.4).
    if [
        "component",
        "driver",
        "monitor",
        "sequencer",
        "agent",
        "env",
        "scoreboard",
        "subscriber",
        "_port",
        "_export",
        "_imp",
        "_fifo",
        "uvm_phase",
        "uvm_domain",
        "uvm_reg",
        "uvm_mem",
        "uvm_vreg",
        "uvm_pool",
        "uvm_queue",
    ]
    .iter()
    .any(|k| low.contains(k))
    {
        return false;
    }
    true
}

pub(crate) fn getter_leak(c: &Cst, s: &mut Sink) {
    for cls in class_decls(c) {
        let mut objects: HashSet<String> = HashSet::new();
        for d in data_items(c, cls) {
            let Some(dt) = c.tree.child(d, DataType) else { continue };
            if !is_cloneable_object_type(c, dt) {
                continue;
            }
            for (dec, n, _) in declarators(c, d) {
                // Unpacked arrays / queues (`T m_bins[$]`) are copied by value on return in SV.
                if c.tree.child(dec, Dim).is_some() {
                    continue;
                }
                objects.insert(n);
            }
        }
        if objects.is_empty() {
            continue;
        }
        for f in methods(c, cls) {
            let name = routine_name(c, f);
            let b = name.as_bytes();
            if !(name.starts_with("get_") || (name.starts_with("get") && b.len() > 3 && b[3].is_ascii_uppercase())) {
                continue;
            }
            for r in crate::support::class_items::subtree(c, f) {
                if c.kind(r) != ReturnStmt {
                    continue;
                }
                let Some(v) = c.tree.children(r).next() else { continue };
                if c.kind(v) != NameExpr {
                    continue;
                }
                let o = own(c, v);
                let [var] = o.as_slice() else { continue };
                if !objects.contains(*var) {
                    continue;
                }
                let Some(kw) = c.tok(r, "return") else { continue };
                s.add(
                    "SVL-OOP-017",
                    "MutableObjectGetterLeak",
                    Severity::Info,
                    format!(
                        "Getter method '{name}()' returns a direct handle to internal object '{var}'. \
                         Callers mutating the returned handle will modify this class's internal state; \
                         return a cloned copy (e.g. 'return {var}.clone();') when defensive isolation is required."
                    ),
                    c.loc(kw),
                );
            }
        }
    }
}

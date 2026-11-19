//! Inter-procedural class-hierarchy call-graph summaries for constructor
//! virtual dispatch (`SV-OOP-017`).

use std::collections::{HashSet, VecDeque};

use dv_model::{Class, Design, Loc, Method};

/// A transitive virtual call reachable from `new()` through a non-virtual helper.
#[derive(Clone, Debug)]
pub(crate) struct CtorTransitiveVirtualCall {
    pub class_name: String,
    pub helper_name: String,
    pub virtual_name: String,
    pub call_loc: Loc,
}

fn find_method_in_lineage<'a>(d: &'a Design, start: &'a Class, name: &str) -> Option<(&'a Class, &'a Method)> {
    let mut cur = Some(start);
    let mut guard = 0usize;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        if let Some(m) = c.methods.iter().find(|m| !m.is_builtin && m.name == name) {
            return Some((c, m));
        }
        cur = c.base.map(|bid| d.class(bid));
    }
    None
}

fn is_self_call(receiver: &str) -> bool {
    receiver.is_empty() || receiver == "this"
}

/// Finds `new()` calls to non-virtual helper methods on `this` that transitively
/// invoke a virtual method (which `SV-OOP-005` misses because `SV-OOP-005` only
/// checks direct calls in `new()`).
pub(crate) fn ctor_transitive_virtual_calls(d: &Design) -> Vec<CtorTransitiveVirtualCall> {
    let mut out = Vec::new();
    for cls in d.classes() {
        if cls.is_library || cls.is_parameterized {
            continue;
        }
        let Some(ctor) = cls.methods.iter().find(|m| !m.is_builtin && !m.is_extern && m.name == "new") else {
            continue;
        };
        for top_call in &ctor.calls {
            if top_call.is_system
                || top_call.callee_builtin
                || top_call.via_super
                || top_call.callee_declared_virtual
                || !is_self_call(&top_call.receiver)
                || top_call.name == "new"
            {
                continue;
            }
            let Some((_, helper_m)) = find_method_in_lineage(d, cls, &top_call.name) else {
                continue;
            };
            if helper_m.is_virtual || helper_m.declared_virtual {
                // Direct virtual call in `new()` belongs to SV-OOP-005.
                continue;
            }
            if let Some(virt_name) = reaches_virtual_method(d, cls, &top_call.name) {
                out.push(CtorTransitiveVirtualCall {
                    class_name: cls.name.clone(),
                    helper_name: top_call.name.clone(),
                    virtual_name: virt_name,
                    call_loc: top_call.loc,
                });
            }
        }
    }
    out
}

fn reaches_virtual_method(d: &Design, root_cls: &Class, start_helper: &str) -> Option<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut q: VecDeque<(String, usize)> = VecDeque::new();
    visited.insert(start_helper.to_string());
    q.push_back((start_helper.to_string(), 0));
    while let Some((cur_name, depth)) = q.pop_front() {
        if depth >= 8 {
            continue;
        }
        let Some((_, m)) = find_method_in_lineage(d, root_cls, &cur_name) else {
            continue;
        };
        for call in &m.calls {
            if call.is_system || call.callee_builtin || call.via_super || !is_self_call(&call.receiver) {
                continue;
            }
            if call.callee_declared_virtual {
                return Some(call.name.clone());
            }
            if let Some((_, target)) = find_method_in_lineage(d, root_cls, &call.name) {
                if target.is_virtual || target.declared_virtual {
                    return Some(call.name.clone());
                }
                if visited.insert(call.name.clone()) {
                    q.push_back((call.name.clone(), depth + 1));
                }
            }
        }
    }
    None
}

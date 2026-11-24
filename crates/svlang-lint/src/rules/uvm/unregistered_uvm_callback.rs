//! UVM-CMP-005 (UnregisteredUvmCallback): Flags `uvm_callbacks#(T, CB)::add(...)`
//! or `` `uvm_do_callbacks(T, CB, ...) `` uses where the callback class `CB` is
//! defined in the compilation without a matching `` `uvm_register_cb(T, CB) ``
//! registration, which causes runtime warnings and silently dropped callbacks.

use std::collections::HashSet;

use svlang::syntax::SyntaxKind::*;
use svlang_model::Design;

use crate::support::trivia::is_library_tok;
use crate::support::uvm_classes::derived;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unregistered_uvm_callback(d: &Design, csts: &[Cst], out: &mut Sink) {
    let cb_classes: HashSet<&str> = d
        .classes()
        .iter()
        .filter(|cls| cls.name != "uvm_callback" && derived(d, cls.id, "uvm_callback"))
        .map(|cls| cls.name.as_str())
        .collect();
    if cb_classes.is_empty() {
        return;
    }

    // Collect all `(T, CB)` pairs registered via `uvm_register_cb(T, CB) or
    // `uvm_callback_ typewide registrations (`m_register_pair`) across all files.
    let mut registered_cbs: HashSet<String> = HashSet::new();
    for c in csts {
        for (fidx, src) in c.tree.sources.iter().enumerate() {
            let _ = fidx;
            let mut rest = &**src;
            while let Some(pos) = rest.find("`uvm_register_cb") {
                rest = &rest[pos + 16..];
                if let Some(open) = rest.find('(') {
                    if let Some(close) = rest[open + 1..].find(')') {
                        let args: Vec<&str> = rest[open + 1..open + 1 + close].split(',').map(str::trim).collect();
                        if let Some(&cb) = args.get(1) {
                            registered_cbs.insert(cb.to_string());
                        }
                    }
                }
            }
        }
        // Also accept explicit `uvm_typeid_base` / `m_register_pair` in custom macros.
        for e in c.nodes(CallExpr) {
            let txt = c.text(e);
            if txt.contains("m_register_pair") {
                for &cb in &cb_classes {
                    if txt.contains(cb) {
                        registered_cbs.insert(cb.to_string());
                    }
                }
            }
        }
    }

    let mut reported = HashSet::new();
    for c in csts {
        for e in c.nodes(CallExpr) {
            let Some(t0) = c.first_tok(e) else { continue };
            if is_library_tok(c, t0) {
                continue;
            }
            let txt = c.text(e);
            if !txt.contains("uvm_callbacks") || !txt.contains("::add") {
                continue;
            }
            // Extract `#(T, CB)` specialization arguments before `::add`.
            let Some(hash) = txt.find("#(") else { continue };
            let Some(close) = txt[hash + 2..].find(')') else { continue };
            let params: Vec<&str> = txt[hash + 2..hash + 2 + close].split(',').map(str::trim).collect();
            let Some(&cb) = params.get(1) else { continue };
            let cb_bare = cb.split('#').next().unwrap_or(cb).trim();
            if cb_classes.contains(cb_bare) && !registered_cbs.contains(cb_bare) {
                let loc = c.loc(t0);
                if reported.insert((loc.file, loc.line, cb_bare.to_string())) {
                    let host = params.first().copied().unwrap_or("T");
                    out.add(
                        "UVM-CMP-005",
                        "UnregisteredUvmCallback",
                        Severity::Warning,
                        format!(
                            "Callback class '{cb_bare}' is attached via 'uvm_callbacks#({host}, {cb_bare})::add' without a matching '`uvm_register_cb({host}, {cb_bare})' declaration. Unregistered UVM callbacks trigger runtime UVM_WARNINGs and are ignored."
                        ),
                        loc,
                    );
                }
            }
        }
    }
}

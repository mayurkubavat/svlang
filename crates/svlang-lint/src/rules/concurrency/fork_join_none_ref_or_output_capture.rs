//! SV-CON-018 (ForkJoinNoneRefOrOutputCapture): Flags `fork ... join_none`
//! background threads spawned inside tasks or functions that reference `ref`,
//! `const ref`, `output`, or `inout` formal arguments of the enclosing routine.
//! Because `join_none` detaches immediately, the parent routine may return
//! while the child thread still reads or writes the caller's argument binding
//! (IEEE 1800 §9.3.2 & §13.5.2 lifetime race).

use std::collections::HashSet;

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn collect_ref_or_out_args(c: &Cst, decl: NodeId) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    if let Some(pl) = c.tree.child(decl, PortList) {
        let mut sticky_dir: &'static str = "input";
        for p in c.tree.children(pl).filter(|&k| c.kind(k) == Port) {
            let toks: Vec<&str> = c.toks(p).map(|t| c.text_of(t)).collect();
            if toks.contains(&"ref") {
                sticky_dir = if toks.contains(&"const") { "const ref" } else { "ref" };
            } else if toks.contains(&"output") {
                sticky_dir = "output";
            } else if toks.contains(&"inout") {
                sticky_dir = "inout";
            } else if toks.contains(&"input") || c.tree.child(p, DataType).is_some() {
                sticky_dir = "input";
            }
            if sticky_dir == "input" {
                continue;
            }
            for d in c.tree.children(p).filter(|&d| c.kind(d) == Declarator) {
                if let Some(nm) = c.tree.child(d, Name) {
                    out.push((c.tree.bare_text(nm), sticky_dir));
                }
            }
        }
    }
    if let Some(body) = c.tree.child(decl, Body) {
        for dd in c.tree.children(body).filter(|&k| c.kind(k) == DataDecl) {
            let first = c.toks(dd).next().map(|t| c.text_of(t)).unwrap_or("");
            let dir = match first {
                "ref" => "ref",
                "output" => "output",
                "inout" => "inout",
                _ => continue,
            };
            for d in c.tree.children(dd).filter(|&d| c.kind(d) == Declarator) {
                if let Some(nm) = c.tree.child(d, Name) {
                    out.push((c.tree.bare_text(nm), dir));
                }
            }
        }
    }
    out
}

pub(crate) fn fork_join_none_ref_or_output_capture(c: &Cst, out: &mut Sink) {
    for decl in c.nodes(TaskDecl).into_iter().chain(c.nodes(FunctionDecl)) {
        let Some(body) = c.tree.child(decl, Body) else { continue };
        let args = collect_ref_or_out_args(c, decl);
        if args.is_empty() {
            continue;
        }
        let rname = c.tree.child(decl, Name).map(|n| c.tree.bare_text(n)).unwrap_or_default();

        for pb in c.nodes(ParBlock) {
            if c.last_text(pb) != "join_none" {
                continue;
            }
            if !std::iter::successors(Some(pb), |&x| c.parent(x)).any(|x| x == body) {
                continue;
            }
            let Some(fork_tok) = c.first_tok(pb) else { continue };
            if is_library_tok(c, fork_tok) {
                continue;
            }

            // Exclude names shadowed by `fork`'s own leading `DataDecl` copies (`automatic T local_copy = arg;`).
            let mut fork_locals: HashSet<String> = HashSet::new();
            for k in c.tree.children(pb).filter(|&k| c.kind(k) == DataDecl) {
                for d in c.tree.children(k).filter(|&d| c.kind(d) == Declarator) {
                    if let Some(nm) = c.tree.child(d, Name) {
                        fork_locals.insert(c.tree.bare_text(nm));
                    }
                }
            }

            let mut used_in_threads: HashSet<String> = HashSet::new();
            for item in c.tree.children(pb) {
                if matches!(c.kind(item), DataDecl | Name | EndLabel) {
                    continue;
                }
                for tid in c.tree.token_range(item) {
                    let prev = tid.checked_sub(1).map(|p| c.text_of(p));
                    if matches!(prev, Some("." | "::")) {
                        continue;
                    }
                    let txt = c.text_of(tid);
                    if !fork_locals.contains(txt) {
                        used_in_threads.insert(txt.to_string());
                    }
                }
            }

            for (aname, dir) in &args {
                if used_in_threads.contains(aname) {
                    out.add(
                        "SV-CON-018",
                        "ForkJoinNoneRefOrOutputCapture",
                        Severity::Warning,
                        format!(
                            "Detached 'fork ... join_none' thread in '{rname}' captures {dir} argument '{aname}'. If '{rname}' returns before the spawned process completes, accesses to '{aname}' race with or outlive the caller's variable; copy inputs into an automatic local before spawning or synchronize with 'join'/'wait fork'."
                        ),
                        c.loc(fork_tok),
                    );
                    break;
                }
            }
        }
    }
}

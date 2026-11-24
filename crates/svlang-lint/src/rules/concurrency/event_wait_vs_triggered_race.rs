//! SVL-CON-012 (EventWaitVsTriggeredRace): Flags `@(ev)` / `@ev` named-event
//! waits inside `fork ... join_none` (or `join_any`) child threads when the
//! same event `ev` is triggered with `-> ev` in an enclosing or sibling thread.
//! Because `fork ... join_none` schedules child threads only after the parent
//! suspends (IEEE 1800 §9.3.2), an active-region `-> ev` before suspension is
//! missed by `@(ev)`; `wait(ev.triggered)` observes persistent same-slot state.

use std::collections::HashSet;

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn collect_triggered_events(c: &Cst, scope: NodeId) -> HashSet<String> {
    let mut evs = HashSet::new();
    for n in c.subtree(scope) {
        if c.kind(n) != EventTrigger {
            continue;
        }
        // Only blocking `-> ev` races within the active region; `->> ev` schedules NBA.
        if c.toks(n).any(|t| c.text_of(t) == "->>") {
            continue;
        }
        if let Some(target) = c.tree.children(n).last() {
            let name = c.trimmed(target);
            if !name.is_empty() {
                evs.insert(name);
            }
        }
    }
    evs
}

pub(crate) fn event_wait_vs_triggered_race(c: &Cst, out: &mut Sink) {
    // Check routines (`TaskDecl`) and `ProceduralBlock`s (`initial`, `always`).
    let mut roots: Vec<NodeId> = c.nodes(TaskDecl);
    roots.extend(c.nodes(ProceduralBlock));
    for root in roots {
        let triggered = collect_triggered_events(c, root);
        if triggered.is_empty() {
            continue;
        }
        for pb in c.subtree(root).into_iter().filter(|&n| c.kind(n) == ParBlock) {
            let join = c.last_text(pb);
            if join != "join_none" && join != "join_any" {
                continue;
            }
            for tc in c.subtree(pb).into_iter().filter(|&n| c.kind(n) == TimingControl) {
                let Some(t0) = c.first_tok(tc) else { continue };
                if is_library_tok(c, t0) || c.text_of(t0) != "@" {
                    continue;
                }
                for ev_expr in c.tree.children(tc).filter(|&x| c.kind(x) == EventExpr) {
                    // Skip edge-qualified clock/signal waits (`posedge`, `negedge`, `edge`).
                    if c.toks(ev_expr).any(|t| matches!(c.text_of(t), "posedge" | "negedge" | "edge")) {
                        continue;
                    }
                    let ev_name = c.trimmed(ev_expr);
                    if triggered.contains(&ev_name) {
                        out.add(
                            "SVL-CON-012",
                            "EventWaitVsTriggeredRace",
                            Severity::Warning,
                            format!(
                                "Event control '@({ev_name})' inside 'fork ... {join}' races with same-timestep '-> {ev_name}'. Because '{join}' child threads start only after the parent blocks (IEEE 1800 §9.3.2), '@({ev_name})' can miss the trigger and hang; use 'wait({ev_name}.triggered)' instead."
                            ),
                            c.loc(t0),
                        );
                    }
                }
            }
        }
    }
}

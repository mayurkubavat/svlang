//! SV-TIM-002 (MissingTimescaleInModule): Flags files containing unitless
//! nonzero procedural `#delay` controls that lack an explicit `` `timescale ``
//! or `timeunit` declaration (IEEE 1800-2023 §3.14).

use crate::support::class_items::op;
use crate::support::data_items::main_files;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn has_timescale(src: &str) -> bool {
    for (word, len) in [("`timescale", 10), ("timeunit", 8)] {
        let mut pos = 0;
        while let Some(i) = src[pos..].find(word) {
            let at = pos + i;
            let start = src[..at].rfind('\n').map_or(0, |x| x + 1);
            let prefix = &src[start..at];
            if !prefix.contains("//") && !prefix.contains("/*") {
                return true;
            }
            pos = at + len;
        }
    }
    false
}

/// True when `n` (`TimingControl` with `#`) is a unitless nonzero simulation delay
/// whose meaning depends on `` `timescale `` / `timeunit` (excluding `@(...)` event
/// controls, `##` cycle delays, `#1step`, `#0` / `assert #0` deferred assertions,
/// explicit IEEE 1800 §5.8 time literals like `#5ns` / `#100ps`, and gate/instance
/// `#(...)` parameter/delay lists).
fn is_unitless_hash_delay(c: &Cst, n: NodeId) -> bool {
    if op(c, n) != "#" {
        return false;
    }
    if c.parent(n).is_some_and(|p| matches!(c.kind(p), GateInst | Instantiation | AssertStmt)) {
        return false;
    }
    let raw = c.trimmed(n);
    let body = raw.strip_prefix('#').unwrap_or("").trim().trim_matches(['(', ')', ' ', '\t']);
    if body.is_empty() || body == "0" || body == "0.0" || body == "1step" {
        return false;
    }
    // Explicit IEEE 1800 §5.8 time literals (`#10ns`, `#1.5us`, `#100ps`, `#1ms`, `#1fs`, `#1s`)
    // carry their own unit and do not depend on `timescale.
    for unit in ["fs", "ps", "ns", "us", "ms", "s"] {
        if let Some(num) = body.strip_suffix(unit) {
            let num = num.trim();
            if !num.is_empty() && num.chars().all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '_') {
                return false;
            }
        }
    }
    true
}

pub(crate) fn missing_timescale(c: &Cst, s: &mut Sink) {
    for f in main_files(c) {
        if has_timescale(&c.tree.sources[f as usize]) {
            continue;
        }
        let mut first = None;
        let mut stack = vec![c.tree.root];
        while let Some(n) = stack.pop() {
            match c.kind(n) {
                SequenceDecl | PropertyDecl | SequenceExpr | PropertyExpr | ConcurrentAssert | AssertStmt => {
                    continue;
                }
                TimingControl if is_unitless_hash_delay(c, n) => {
                    first = Some(n);
                    break;
                }
                TimedExpr if c.first_tok(n).is_some_and(|t| c.text_of(t) == "#") => {
                    let raw = c.trimmed(n);
                    let body = raw.strip_prefix('#').unwrap_or("").trim();
                    if !body.starts_with("0 ") && !body.starts_with("1step") {
                        first = Some(n);
                        break;
                    }
                }
                _ => {}
            }
            let k: Vec<NodeId> = c.tree.children(n).collect();
            stack.extend(k.into_iter().rev());
        }
        let Some(at) = first.and_then(|n| c.first_tok(n)) else { continue };
        s.add(
            "SV-TIM-002",
            "MissingTimescaleInModule",
            Severity::Warning,
            "File contains unitless procedural '#delay' controls without a '`timescale' or 'timeunit' declaration. \
             Per IEEE 1800 § 3.14, unitless delays adopt simulator-dependent default time units; add 'timeunit' / '`timescale' or use explicit time literals (e.g. '#10ns')."
                .into(),
            c.loc(at),
        );
    }
}

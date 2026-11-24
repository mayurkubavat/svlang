//! SV-PRF-010 (StringConcatInLoop): Disallows iterative in-place string or
//! unpacked array/queue concatenation (`s = {s, ...}`) or `$sformatf`
//! accumulation inside loop bodies to prevent O(N^2) memory reallocation.

use crate::support::class_items::declarators;
use crate::support::for_loops::{kids, op_text, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

/// Collects variables declared as `string`, dynamic arrays `[]`, or queues `[$]`.
/// Packed bit/logic vectors (`logic [7:0] sr`) perform zero-allocation bit-shifting
/// in `{sr[6:0], in_b}` and must never be flagged as O(N^2) heap concatenations.
fn heap_concat_vars(c: &Cst) -> HashSet<String> {
    let mut out = HashSet::new();
    for dd in c.nodes(DataDecl) {
        let is_str =
            c.tree.child(dd, DataType).and_then(|dt| c.first_tok(dt)).is_some_and(|t| c.text_of(t) == "string");
        for (dec, name, _) in declarators(c, dd) {
            let is_dyn_or_q = c.tree.children(dec).filter(|&k| c.kind(k) == Dim).any(|k| {
                let txt = c.tree.bare_text(k);
                let inner = txt.trim().trim_start_matches('[').trim_end_matches(']').trim();
                inner.is_empty() || inner == "$" || inner.starts_with("$:")
            });
            if is_str || is_dyn_or_q {
                out.insert(name);
            }
        }
    }
    for port in c.nodes(Port) {
        let is_str =
            c.tree.child(port, DataType).and_then(|dt| c.first_tok(dt)).is_some_and(|t| c.text_of(t) == "string");
        if is_str {
            if let Some(nm) = c.tree.child(port, Name) {
                out.insert(c.tree.bare_text(nm));
            }
        }
    }
    out
}

fn expr_mentions_ident(c: &Cst, root: NodeId, var: &str) -> bool {
    c.tree.token_range(root).into_iter().any(|t| c.text_of(t) == var)
}

pub(crate) fn concat_in_loop(c: &Cst, s: &mut Sink) {
    let heap_vars = heap_concat_vars(c);
    if heap_vars.is_empty() {
        return;
    }
    let mut loops = c.nodes(LoopStmt);
    loops.extend(c.nodes(DoWhileStmt));
    loops.sort_by_key(|&n| c.first_tok(n));
    for l in loops {
        let body = match c.keyword(l).map(|k| c.text_of(k)) {
            Some("for" | "foreach" | "while" | "repeat") => kids(c, l).last().copied(),
            Some("do") => kids(c, l).first().copied(),
            _ => None,
        };
        let Some(body) = body else { continue };
        let mut all = Vec::new();
        subtree(c, body, &mut all);
        for n in all {
            if c.kind(n) != BinaryExpr || op_text(c, n) != "=" {
                continue;
            }
            let k = kids(c, n);
            let [left, right] = k[..] else { continue };
            if c.kind(left) != NameExpr {
                continue;
            }
            let var = c.tree.bare_text(left);
            if !heap_vars.contains(&var) {
                continue;
            }
            let Some(op) = c.toks(n).next() else { continue };
            let msg = match c.kind(right) {
                ConcatExpr if is_plain_concat(c, right) && expr_mentions_ident(c, right, &var) => format!(
                    "Iterative concatenation of variable '{var}' inside a loop construct. In SystemVerilog, repeatedly concatenating strings or unpacked arrays inside loops causes quadratic heap allocations and memory copies (O(N^2)). Accumulate elements into a queue ('q.push_back(...)') or pre-allocate fixed buffers instead."
                ),
                CallExpr
                    if kids(c, right).first().is_some_and(|&f| c.last_text(f) == "$sformatf")
                        && expr_mentions_ident(c, right, &var) =>
                {
                    format!(
                        "Iterative string formatting into '{var}' via $sformatf inside a loop construct. Repeatedly reallocating and copying strings inside loops causes memory churn and simulation slowdown. Accumulate messages into a queue or format once outside the loop."
                    )
                }
                _ => continue,
            };
            s.add("SV-PRF-010", "StringConcatInLoop", Severity::Warning, msg, c.loc(op));
        }
    }
}

/// slang's `ConcatenationExpression`: not `{}`, a replication or a stream.
fn is_plain_concat(c: &Cst, n: NodeId) -> bool {
    let k = kids(c, n);
    let replication = k.len() == 2 && c.tok(n, ",").is_none() && c.kind(k[1]) == ConcatExpr;
    !k.is_empty() && !replication && c.tok(n, "<<").is_none() && c.tok(n, ">>").is_none()
}

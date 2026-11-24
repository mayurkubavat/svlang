//! SVL-PRT-003 (GlobalUnitPollution): Disallow variables, nets, typedefs, tasks,
//! and functions declared directly in the $unit compilation unit scope.

use crate::support::ast::{kids, last_tok};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::{NodeId, SyntaxKind};
use SyntaxKind::*;

const NET_TYPES: &[&str] = &[
    "wire",
    "tri",
    "wand",
    "wor",
    "triand",
    "trior",
    "tri0",
    "tri1",
    "trireg",
    "uwire",
    "supply0",
    "supply1",
    "interconnect",
];

pub(crate) fn unit_pollution(c: &Cst, s: &mut Sink) {
    let root = c.tree.root;
    for item in kids(c, root) {
        let (kind, names): (&str, Vec<NodeId>) = match c.kind(item) {
            DataDecl => {
                let net = c.toks(item).any(|t| NET_TYPES.contains(&c.text_of(t)));
                let decls = kids(c, item)
                    .into_iter()
                    .filter(|&d| c.kind(d) == Declarator)
                    .filter_map(|d| c.tree.child(d, Name))
                    .collect();
                (if net { "net" } else { "variable" }, decls)
            }
            TypedefDecl => ("typedef", c.tree.child(item, Name).into_iter().collect()),
            // An out-of-block body (`function C::f`) belongs to its class.
            FunctionDecl | TaskDecl | DpiImport => {
                ("subroutine", c.tree.child(item, Name).filter(|&n| c.tok(n, "::").is_none()).into_iter().collect())
            }
            _ => continue,
        };
        for name in names {
            let Some(t) = last_tok(c, name) else { continue };
            // A `.svh` / `.vh` header opened standalone (as a compilation input rather than
            // `include`d by a `.sv` unit) is normally included inside a `package` or `module`;
            // if a `.sv` file includes a `.svh` at `$unit` scope, `f` is not in `mains`, so
            // genuine `$unit` leaks are still reported.
            let f = c.tree.tokens[t as usize].origin.file;
            if c.tree.mains.contains(&f)
                && c.tree
                    .files
                    .get(f as usize)
                    .and_then(|p| p.extension())
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| matches!(ext, "svh" | "vh"))
            {
                continue;
            }
            s.add(
                "SVL-PRT-003",
                "GlobalUnitPollution",
                Severity::Error,
                format!(
                    "Illegal declaration of {kind} '{}' in $unit compilation unit scope. All shared data types, variables, and routines \
                     must be encapsulated within a package (e.g. 'package my_pkg; ... endpackage') or module.",
                    c.text_of(t)
                ),
                c.loc(t),
            );
        }
    }
}

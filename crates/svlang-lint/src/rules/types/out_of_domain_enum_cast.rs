//! SVL-TYP-005 (OutOfDomainEnumCast): Flags static casts `enum_t'(CONST)` where
//! `CONST` is a compile-time constant integer literal that does not match any
//! declared enumerator value of `enum_t`, producing an out-of-domain enum value
//! that breaks `case` exhaustiveness and waveform decoding.

use crate::support::config_db_calls::strip_parens;
use crate::support::literal_values::literal_value;
use crate::support::slang_ast::kids;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn resolve_enum_values(c: &Cst, target: NodeId) -> Option<(String, Vec<(String, i64)>)> {
    let toks: Vec<u32> = c.toks(target).collect();
    let (pkg, t) = match toks[..] {
        [t] => (None, t),
        [p, sep, t] if c.text_of(sep) == "::" => (Some(c.text_of(p)), t),
        _ => return None,
    };
    let tn = c.text_of(t);
    let scope_of = |n: NodeId| {
        std::iter::successors(c.parent(n), |&a| c.parent(a)).find(|&a| {
            matches!(
                c.kind(a),
                ModuleDecl | InterfaceDecl | ProgramDecl | ClassDecl | PackageDecl | FunctionDecl | TaskDecl
            )
        })
    };
    let around: Vec<NodeId> = std::iter::successors(c.parent(target), |&a| c.parent(a)).collect();
    for tree in c.all.iter().chain(std::iter::once(c.tree).take(if c.all.is_empty() { 1 } else { 0 })) {
        let tc = if std::ptr::eq(tree, c.tree) {
            None
        } else {
            Some(Cst::new(tree, 0, svlang::semantic::Locator::default()))
        };
        let r = tc.as_ref().unwrap_or(c);
        let mut candidates = r
            .nodes(TypedefDecl)
            .into_iter()
            .filter(|&td| r.tree.child(td, Name).is_some_and(|n| r.tree.bare_text(n) == tn))
            .filter(|&td| {
                pkg.is_none_or(|p| {
                    std::iter::successors(r.parent(td), |&a| r.parent(a)).any(|sc| {
                        r.kind(sc) == PackageDecl && r.tree.child(sc, Name).is_some_and(|n| r.tree.bare_text(n) == p)
                    })
                })
            });
        let best = if std::ptr::eq(r.tree, c.tree) {
            candidates
                .filter_map(|td| match scope_of(td) {
                    None => Some((usize::MAX, td)),
                    Some(sc) => match around.iter().position(|&a| a == sc) {
                        Some(depth) => Some((depth, td)),
                        None if c.kind(sc) == PackageDecl => Some((usize::MAX - 1, td)),
                        None => None,
                    },
                })
                .min_by_key(|x| x.0)
                .map(|x| x.1)
        } else {
            candidates
                .find(|&td| std::iter::successors(r.parent(td), |&a| r.parent(a)).any(|sc| r.kind(sc) == PackageDecl))
        };
        let Some(td) = best else { continue };
        let Some(dt) = r.tree.child(td, DataType) else { continue };
        r.tok(dt, "enum")?;
        let mut members = Vec::new();
        let mut next = 0i64;
        for d in r.tree.children(dt).filter(|&d| r.kind(d) == Declarator) {
            let Some(nm) = r.tree.child(d, Name) else { continue };
            let val = match r.tree.children(d).find(|&x| x != nm && r.kind(x) != Dim) {
                Some(e) => literal_value(r, r.strip(e))?,
                None => next,
            };
            members.push((r.tree.bare_text(nm), val));
            next = val.wrapping_add(1);
        }
        if !members.is_empty() {
            return Some((tn.to_string(), members));
        }
    }
    None
}

pub(crate) fn out_of_domain_enum_cast(c: &Cst, out: &mut Sink) {
    for e in c.nodes(CastExpr) {
        let Some(t0) = c.first_tok(e) else { continue };
        if is_library_tok(c, t0) {
            continue;
        }
        let ks = kids(c, e);
        let (Some(&target), Some(&inner)) = (ks.first(), ks.get(1)) else { continue };
        let inner = strip_parens(c, inner);
        if !matches!(c.kind(inner), Literal | PrefixExpr) {
            continue;
        }
        let Some(val) = literal_value(c, inner) else { continue };
        let Some((ename, members)) = resolve_enum_values(c, target) else { continue };
        if !members.iter().any(|(_, mv)| *mv == val) {
            out.add(
                "SVL-TYP-005",
                "OutOfDomainEnumCast",
                Severity::Warning,
                format!(
                    "Static cast '{ename}'({val})' assigns constant {val}, which is not a declared enumerator value of enum '{ename}'. Out-of-domain enum casts create illegal states and bypass case checks."
                ),
                c.loc(t0),
            );
        }
    }
}

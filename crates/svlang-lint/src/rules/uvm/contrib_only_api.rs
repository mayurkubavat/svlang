//! UVM-PRT-002 (ContribOnlyApi): Flags calls to, and classes extending, parts
//! of the Accellera UVM library tagged `@uvm-contrib` (experimental / non-IEEE
//! contributed extensions).

use std::collections::HashSet;

use svlang::syntax::SyntaxKind::*;
use svlang_model::{Design, Loc};

use crate::support::defines_packages::{api_pack, user_tok, Files};
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn contrib_only_api(_: &Design, csts: &[Cst], out: &mut Sink) {
    let Some(pack) = api_pack(csts) else { return };
    let report = |out: &mut Sink, what: String, where_: &str, loc: Loc| {
        out.add(
            "UVM-PRT-002",
            "ContribOnlyApi",
            Severity::Info,
            format!(
                "{what} ({where_}) is tagged @uvm-contrib in UVM {}, an unstandardized contribution outside IEEE 1800.2. \
Prefer standard IEEE 1800.2 APIs so testbenches remain portable across UVM implementations.",
                pack.version
            ),
            loc,
        );
    };
    let mut seen = HashSet::new();
    for c in csts {
        let fs = Files::new(c);
        for e in c.nodes(CallExpr) {
            let Some(t) = c.first_tok(e) else { continue };
            if c.text_of(t).starts_with('$') || !user_tok(c, &fs, t) {
                continue;
            }
            let Some(decl) = crate::support::slang_ast::callee_decl(c, e) else { continue };
            if !matches!(c.kind(decl), FunctionDecl | TaskDecl | MethodPrototype) {
                continue;
            }
            let Some(cls) = crate::support::slang_ast::ancestors(c, decl).find(|&a| c.kind(a) == ClassDecl) else {
                continue;
            };
            let cname = crate::support::slang_ast::decl_name(c, cls);
            let member = crate::support::slang_ast::last_name(c, decl);
            if !cname.starts_with("uvm_") || !pack.contrib_members.contains(&(cname.clone(), member.clone())) {
                continue;
            }
            let o = c.tree.tokens[t as usize].origin;
            if !seen.insert((fs.path(o.file), o.offset)) {
                continue;
            }
            report(out, format!("'{cname}::{member}'"), "called here", c.loc(t));
        }
        for k in c.nodes(ClassDecl) {
            let Some(nm) = c.tree.child(k, Name) else { continue };
            let Some((_, nt)) = c.tree.token_bounds(nm) else { continue };
            if !user_tok(c, &fs, nt) {
                continue;
            }
            let Some(bt) = crate::support::uvm_syntax::base_name_node(c, k) else { continue };
            let Some(base) = c
                .toks(bt)
                .map(|t| c.text_of(t))
                .take_while(|t| *t != "#")
                .filter(|t| t.chars().next().is_some_and(|ch| ch.is_alphabetic() || ch == '_'))
                .last()
                .map(str::to_string)
            else {
                continue;
            };
            if !pack.contrib_classes.contains(&base) {
                continue;
            }
            let name = c.tree.bare_text(nm);
            for _ in 0..crate::support::style_config::ast_visits(c, nm) {
                report(out, format!("class '{base}'"), &format!("'{name}' extends it"), c.loc(nt));
            }
        }
    }
}

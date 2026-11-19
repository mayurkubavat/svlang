//! Intra-procedural control-flow and definite-assignment dataflow over CST
//! subroutine bodies (`SV-DAT-001`, `SV-DAT-002`, `SV-DAT-003`).

use std::collections::HashSet;

use dv_model::Loc;
use svlang::syntax::{NodeId, SyntaxKind::*};

use crate::support::class_items::{kids, user_code};
use crate::support::procedural::{is_stmt, items, loop_body, loop_kw};
use crate::support::sema_facts::ast_ok;
use crate::support::slang_ast::{ancestors, class_chain, class_routines, enclosing_class, last_name};
use crate::syntax::Cst;

#[derive(Clone, Debug)]
pub(crate) struct MissingReturn {
    pub func_name: String,
    pub ret_type: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct UninitRead {
    pub routine_name: String,
    pub var_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct UnwrittenOutput {
    pub routine_name: String,
    pub arg_name: String,
    pub loc: Loc,
}

#[derive(Default)]
pub(crate) struct CfgFacts {
    pub missing_returns: Vec<MissingReturn>,
    pub uninit_reads: Vec<UninitRead>,
    pub unwritten_outputs: Vec<UnwrittenOutput>,
}

pub(crate) fn facts(c: &Cst) -> std::sync::Arc<CfgFacts> {
    crate::shared::get("cfg_facts", c.tree_key(), || analyze_cst(c))
}

fn analyze_cst(c: &Cst) -> CfgFacts {
    let mut facts = CfgFacts::default();
    for decl in c.nodes(FunctionDecl).into_iter().chain(c.nodes(TaskDecl)) {
        if !user_code(c, decl) || !ast_ok(c, decl) {
            continue;
        }
        let Some(tok) = c.first_tok(decl) else { continue };
        if c.loc(tok).expansion.is_some() {
            continue;
        }
        let Some(body) = c.tree.child(decl, Body) else { continue };
        // Skip routines containing unexpanded/opaque macro statements that may return or write arguments.
        if has_macro_stmt(c, body) {
            continue;
        }
        let rname = last_name(c, decl);
        if rname.is_empty() || rname == "new" {
            continue;
        }
        if c.kind(decl) == FunctionDecl {
            check_missing_return(c, decl, body, &rname, &mut facts.missing_returns);
        }
        check_uninit_locals(c, decl, body, &rname, &mut facts.uninit_reads);
        check_unwritten_outputs(c, decl, body, &rname, &mut facts.unwritten_outputs);
    }
    facts
}

fn has_macro_stmt(c: &Cst, body: NodeId) -> bool {
    c.toks(body).any(|t| {
        let txt = c.text_of(t);
        txt.starts_with('`') && !matches!(txt, "`uvm_info" | "`uvm_warning" | "`uvm_error")
    })
}

fn check_missing_return(c: &Cst, decl: NodeId, body: NodeId, fname: &str, out: &mut Vec<MissingReturn>) {
    let Some(rt_node) = c.tree.child(decl, ReturnType) else { return };
    let rt_text = c.tree.bare_text(rt_node);
    let rt_trim = rt_text.trim();
    if rt_trim.is_empty() || rt_trim == "void" {
        return;
    }
    // Skip empty stub functions (already covered by SV-DED-011 / virtual stubs).
    let stmts = items(c, body);
    if stmts.is_empty() {
        return;
    }
    // In IEEE 1800 §13.4.1, assigning to the function's own identifier sets its return value.
    if assigns_ident_anywhere(c, body, fname) {
        return;
    }
    if !block_must_terminate(c, &stmts) {
        let name_tok =
            c.tree.child(decl, Name).and_then(|n| c.tree.token_bounds(n).map(|b| b.1)).or_else(|| c.first_tok(decl));
        if let Some(t) = name_tok {
            out.push(MissingReturn { func_name: fname.to_string(), ret_type: rt_trim.to_string(), loc: c.loc(t) });
        }
    }
}

fn block_must_terminate(c: &Cst, stmts: &[NodeId]) -> bool {
    stmts.iter().any(|&s| stmt_must_terminate(c, s))
}

fn stmt_must_terminate(c: &Cst, s: NodeId) -> bool {
    match c.kind(s) {
        ReturnStmt => true,
        SeqBlock => block_must_terminate(c, &items(c, s)),
        IfStmt => {
            let ks = kids(c, s);
            let Some(&then_s) = ks.iter().find(|&&k| is_stmt(c.kind(k))) else { return false };
            let Some(else_cl) = c.tree.child(s, ElseClause) else { return false };
            let Some(else_s) = c.tree.children(else_cl).find(|&k| is_stmt(c.kind(k))) else { return false };
            stmt_must_terminate(c, then_s) && stmt_must_terminate(c, else_s)
        }
        CaseStmt => {
            let mut has_default = false;
            let mut arm_stmts = Vec::new();
            let mut opened = false;
            for ch in &c.tree.node(s).children {
                match *ch {
                    svlang::syntax::Child::Token(t) => match c.text_of(t) {
                        ")" => opened = true,
                        "default" => has_default = true,
                        _ => {}
                    },
                    svlang::syntax::Child::Node(n) if opened && is_stmt(c.kind(n)) => {
                        arm_stmts.push(n);
                    }
                    _ => {}
                }
            }
            let unique_or_priority =
                c.toks(s).take(3).any(|t| matches!(c.text_of(t), "unique" | "unique0" | "priority"));
            !arm_stmts.is_empty()
                && (has_default || unique_or_priority)
                && arm_stmts.into_iter().all(|a| stmt_must_terminate(c, a))
        }
        LoopStmt if loop_kw(c, s) == "forever" => !contains_break(c, s),
        ExprStmt => {
            let txt = c.tree.bare_text(s);
            txt.starts_with("$fatal") || txt.contains("uvm_report_fatal") || txt.contains("`uvm_fatal")
        }
        _ => false,
    }
}

fn contains_break(c: &Cst, root: NodeId) -> bool {
    for n in c.tree.children(root) {
        if matches!(c.kind(n), LoopStmt | DoWhileStmt) {
            continue;
        }
        if c.kind(n) == JumpStmt && c.first_tok(n).is_some_and(|t| c.text_of(t) == "break") {
            return true;
        }
        if contains_break(c, n) {
            return true;
        }
    }
    false
}

fn assigns_ident_anywhere(c: &Cst, root: NodeId, target: &str) -> bool {
    for n in crate::support::slang_ast::subtree(c, root) {
        if c.kind(n) == BinaryExpr && is_any_assign_op(c, n) {
            if let Some(&lhs) = kids(c, n).first() {
                if root_ident(c, lhs).as_deref() == Some(target) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_any_assign_op(c: &Cst, e: NodeId) -> bool {
    let Some(kw) = c.keyword(e) else { return false };
    let op = c.text_of(kw);
    if op == "<=" {
        return c.parent(e).is_some_and(|p| c.kind(p) == ExprStmt);
    }
    matches!(op, "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | "<<<=" | ">>>=")
}

fn is_plain_assign_op(c: &Cst, e: NodeId) -> bool {
    let Some(kw) = c.keyword(e) else { return false };
    let op = c.text_of(kw);
    op == "=" || (op == "<=" && c.parent(e).is_some_and(|p| c.kind(p) == ExprStmt))
}

fn root_ident(c: &Cst, mut e: NodeId) -> Option<String> {
    loop {
        match c.kind(e) {
            ParenExpr | SelectExpr | MemberExpr => {
                e = *kids(c, e).first()?;
            }
            NameExpr => {
                let t = c.first_tok(e)?;
                return Some(c.text_of(t).to_string());
            }
            ScopedExpr => {
                return Some(c.last_text(e).to_string());
            }
            _ => return None,
        }
    }
}

fn is_scalar_or_handle_type(c: &Cst, dt: Option<NodeId>) -> bool {
    let Some(d) = dt else { return true };
    let first = c.first_tok(d).map(|t| c.text_of(t)).unwrap_or("");
    if matches!(first, "struct" | "union" | "event") {
        return false;
    }
    // Skip typedefs likely naming unpacked structs (`*_s`, `*_struct`) where field-by-field writes are idiomatic;
    // keep scalar/integral/string/class types. Actually field writes `v.f = ...` already mark `v` initialized!
    // Only skip `event` (events are triggered with `-> ev` and waited on with `@(ev)`, never "assigned").
    true
}

fn check_uninit_locals(c: &Cst, decl: NodeId, body: NodeId, rname: &str, out: &mut Vec<UninitRead>) {
    // Only automatic routines (class methods are automatic by IEEE 1800 §8.6; free routines if `automatic`).
    let in_class = ancestors(c, decl).any(|a| c.kind(a) == ClassDecl);
    let explicit_static = c.toks(decl).take_while(|&t| c.text_of(t) != "(").any(|t| c.text_of(t) == "static");
    let explicit_auto = c.toks(decl).take_while(|&t| c.text_of(t) != "(").any(|t| c.text_of(t) == "automatic");
    if explicit_static || (!in_class && !explicit_auto) {
        return;
    }
    // Collect top-level body automatic scalar/handle locals without initializers.
    let mut tracked: HashSet<String> = HashSet::new();
    for item in c.tree.children(body).filter(|&k| c.kind(k) == DataDecl) {
        let own_static = c.toks(item).any(|t| matches!(c.text_of(t), "static" | "const" | "typedef"));
        if own_static {
            continue;
        }
        let dt = c.tree.child(item, DataType);
        if !is_scalar_or_handle_type(c, dt) {
            continue;
        }
        // Skip typedefs of structs/events whose declaration has unpacked/struct members when untouched:
        // Wait: `v.field = 1` marks `v` initialized below anyway! Skip unpacked arrays (`Dim` on `Declarator`).
        let dt_txt = dt.map(|d| c.tree.bare_text(d)).unwrap_or_default();
        if dt_txt.contains("semaphore") || dt_txt.contains("mailbox") || dt_txt.contains("event") {
            continue;
        }
        // Also skip user struct typedefs (whose fields may be passed or defaulted, or unpacked structs).
        if let Some(d) = dt {
            if is_struct_or_union_typedef(c, d) {
                continue;
            }
        }
        for dec in c.tree.children(item).filter(|&k| c.kind(k) == Declarator) {
            if c.tree.child(dec, Dim).is_some() {
                continue;
            }
            let has_init = kids(c, dec).iter().any(|&k| !matches!(c.kind(k), Name | Dim));
            if has_init {
                continue;
            }
            if let Some(nm) = c.tree.child(dec, Name) {
                let vname = c.tree.bare_text(nm);
                if !vname.is_empty() && vname != rname {
                    tracked.insert(vname);
                }
            }
        }
    }
    if tracked.is_empty() {
        return;
    }
    let mut may_init: HashSet<String> = HashSet::new();
    let mut reported: HashSet<String> = HashSet::new();
    for s in items(c, body) {
        if c.kind(s) == DataDecl {
            continue;
        }
        walk_stmt_uninit(c, s, &tracked, &mut may_init, &mut reported, rname, out);
    }
}

fn is_struct_or_union_typedef(c: &Cst, dt: NodeId) -> bool {
    let o = svlang::semantic::oracle::TypeOracle::new(c.all);
    let info = o.data_type(c.index(), dt);
    info.text.starts_with("struct") || info.text.starts_with("union") || info.text == "event"
}

fn mark_all_written_in_subtree(c: &Cst, root: NodeId, tracked: &HashSet<String>, may_init: &mut HashSet<String>) {
    for n in crate::support::slang_ast::subtree(c, root) {
        match c.kind(n) {
            BinaryExpr if is_any_assign_op(c, n) => {
                if let Some(&lhs) = kids(c, n).first() {
                    if let Some(id) = root_ident(c, lhs) {
                        if tracked.contains(&id) {
                            may_init.insert(id);
                        }
                    }
                }
            }
            PrefixExpr | PostfixExpr if c.keyword(n).is_some_and(|k| matches!(c.text_of(k), "++" | "--")) => {
                if let Some(&opnd) = kids(c, n).first() {
                    if let Some(id) = root_ident(c, opnd) {
                        if tracked.contains(&id) {
                            may_init.insert(id);
                        }
                    }
                }
            }
            CallExpr => {
                for a in crate::expr_types::call_args(c, n) {
                    if let Some(id) = root_ident(c, a) {
                        if tracked.contains(&id) {
                            may_init.insert(id);
                        }
                    }
                }
            }
            EventTrigger => {
                for k in kids(c, n) {
                    if let Some(id) = root_ident(c, k) {
                        if tracked.contains(&id) {
                            may_init.insert(id);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn walk_stmt_uninit(
    c: &Cst,
    s: NodeId,
    tracked: &HashSet<String>,
    may_init: &mut HashSet<String>,
    reported: &mut HashSet<String>,
    rname: &str,
    out: &mut Vec<UninitRead>,
) {
    match c.kind(s) {
        DataDecl => {
            // Nested block declaration: if its name shadows a tracked local, mark it initialized so inner uses aren't attributed!
            for dec in c.tree.children(s).filter(|&k| c.kind(k) == Declarator) {
                if let Some(nm) = c.tree.child(dec, Name) {
                    let v = c.tree.bare_text(nm);
                    if tracked.contains(&v) {
                        may_init.insert(v);
                    }
                }
            }
        }
        SeqBlock => {
            for sub in items(c, s) {
                walk_stmt_uninit(c, sub, tracked, may_init, reported, rname, out);
            }
        }
        IfStmt => {
            let ks = kids(c, s);
            if let Some(&cond) = ks.first().filter(|&&k| !is_stmt(c.kind(k))) {
                walk_expr_uninit(c, cond, tracked, may_init, reported, rname, out);
            }
            let mut branch_init = may_init.clone();
            if let Some(&then_s) = ks.iter().find(|&&k| is_stmt(c.kind(k))) {
                let mut then_set = may_init.clone();
                walk_stmt_uninit(c, then_s, tracked, &mut then_set, reported, rname, out);
                branch_init.extend(then_set);
            }
            if let Some(else_cl) = c.tree.child(s, ElseClause) {
                if let Some(else_s) = c.tree.children(else_cl).find(|&k| is_stmt(c.kind(k))) {
                    let mut else_set = may_init.clone();
                    walk_stmt_uninit(c, else_s, tracked, &mut else_set, reported, rname, out);
                    branch_init.extend(else_set);
                }
            }
            *may_init = branch_init;
        }
        CaseStmt => {
            let mut opened = false;
            let mut sel_checked = false;
            let mut merged = may_init.clone();
            for ch in &c.tree.node(s).children {
                match *ch {
                    svlang::syntax::Child::Token(t) if c.text_of(t) == "(" => opened = true,
                    svlang::syntax::Child::Node(n) if opened && !sel_checked => {
                        sel_checked = true;
                        walk_expr_uninit(c, n, tracked, may_init, reported, rname, out);
                    }
                    svlang::syntax::Child::Node(n) if sel_checked && is_stmt(c.kind(n)) => {
                        let mut arm_set = may_init.clone();
                        walk_stmt_uninit(c, n, tracked, &mut arm_set, reported, rname, out);
                        merged.extend(arm_set);
                    }
                    _ => {}
                }
            }
            *may_init = merged;
        }
        LoopStmt | DoWhileStmt | ParBlock => {
            //Conservatively pre-seed anything written inside the loop/fork so loop-carried writes never false-fire,
            // EXCEPT check a `for` loop's initializer in sequential order first!
            if c.kind(s) == LoopStmt && loop_kw(c, s) == "for" {
                if let Some(&init_n) = kids(c, s).first() {
                    if !is_stmt(c.kind(init_n)) {
                        walk_expr_uninit(c, init_n, tracked, may_init, reported, rname, out);
                    }
                }
            }
            mark_all_written_in_subtree(c, s, tracked, may_init);
            if let Some(lb) = loop_body(c, s) {
                walk_stmt_uninit(c, lb, tracked, may_init, reported, rname, out);
            }
        }
        ExprStmt | ReturnStmt => {
            for k in kids(c, s) {
                walk_expr_uninit(c, k, tracked, may_init, reported, rname, out);
            }
        }
        _ => {
            mark_all_written_in_subtree(c, s, tracked, may_init);
        }
    }
}

fn walk_expr_uninit(
    c: &Cst,
    e: NodeId,
    tracked: &HashSet<String>,
    may_init: &mut HashSet<String>,
    reported: &mut HashSet<String>,
    rname: &str,
    out: &mut Vec<UninitRead>,
) {
    let ks = kids(c, e);
    match c.kind(e) {
        BinaryExpr if is_plain_assign_op(c, e) => {
            if let Some(&rhs) = ks.get(1) {
                walk_expr_uninit(c, rhs, tracked, may_init, reported, rname, out);
            }
            if let Some(&lhs) = ks.first() {
                // Any index expressions inside `arr[idx] = ...` are read!
                if c.kind(lhs) == SelectExpr {
                    for idx in ks_tail(c, lhs) {
                        walk_expr_uninit(c, idx, tracked, may_init, reported, rname, out);
                    }
                }
                if let Some(id) = root_ident(c, lhs) {
                    if tracked.contains(&id) {
                        may_init.insert(id);
                    }
                }
            }
        }
        BinaryExpr if is_any_assign_op(c, e) => {
            // Compound assignment `v += rhs` reads `v` BEFORE writing `v`!
            for &k in &ks {
                walk_expr_uninit(c, k, tracked, may_init, reported, rname, out);
            }
            if let Some(&lhs) = ks.first() {
                if let Some(id) = root_ident(c, lhs) {
                    if tracked.contains(&id) {
                        may_init.insert(id);
                    }
                }
            }
        }
        CallExpr => {
            // Check receiver (if `v.method()`): calling `.randomize()` initializes `v`, other methods read `v`.
            if let Some(&callee) = ks.first() {
                if c.kind(callee) == MemberExpr {
                    let mname = c.last_text(callee);
                    if let Some(&recv) = kids(c, callee).first() {
                        if mname == "randomize" {
                            if let Some(id) = root_ident(c, recv) {
                                if tracked.contains(&id) {
                                    may_init.insert(id);
                                }
                            }
                        } else {
                            walk_expr_uninit(c, recv, tracked, may_init, reported, rname, out);
                        }
                    }
                }
            }
            // Any bare or root-ident call argument could be `output`/`inout`/`ref` (e.g. `get(v)`, `$cast(v, x)`):
            // mark root_ident arguments as potentially initialized, while checking compound sub-expressions like `foo(v + 1)`!
            for a in crate::expr_types::call_args(c, e) {
                if matches!(c.kind(a), NameExpr | MemberExpr | SelectExpr) {
                    if let Some(id) = root_ident(c, a) {
                        if tracked.contains(&id) {
                            may_init.insert(id);
                            continue;
                        }
                    }
                }
                walk_expr_uninit(c, a, tracked, may_init, reported, rname, out);
            }
        }
        NameExpr => {
            if let Some(t) = c.first_tok(e) {
                let name = c.text_of(t);
                if tracked.contains(name) && !may_init.contains(name) && reported.insert(name.to_string()) {
                    out.push(UninitRead { routine_name: rname.to_string(), var_name: name.to_string(), loc: c.loc(t) });
                }
            }
        }
        MemberExpr => {
            if let Some(&base) = ks.first() {
                walk_expr_uninit(c, base, tracked, may_init, reported, rname, out);
            }
        }
        _ => {
            for k in ks {
                walk_expr_uninit(c, k, tracked, may_init, reported, rname, out);
            }
        }
    }
}

fn ks_tail(c: &Cst, select: NodeId) -> Vec<NodeId> {
    kids(c, select).into_iter().skip(1).collect()
}

fn check_unwritten_outputs(c: &Cst, decl: NodeId, body: NodeId, rname: &str, out: &mut Vec<UnwrittenOutput>) {
    // Skip virtual methods, overrides of base class methods, and empty stub bodies.
    if c.toks(decl).take_while(|&t| c.text_of(t) != "(").any(|t| matches!(c.text_of(t), "virtual" | "pure" | "extern"))
    {
        return;
    }
    if let Some(cls) = enclosing_class(c, decl) {
        for anc in class_chain(c, cls).into_iter().skip(1) {
            if class_routines(c, anc).iter().any(|(n, _)| n == rname) {
                return;
            }
        }
    }
    if items(c, body).is_empty() {
        return;
    }
    let Some(pl) = c.tree.child(decl, PortList) else { return };
    let mut outputs: Vec<(String, Loc)> = Vec::new();
    let mut cur_dir = "input";
    for p in c.tree.children(pl).filter(|&k| c.kind(k) == Port) {
        for tok in c.toks(p) {
            match c.text_of(tok) {
                "input" => cur_dir = "input",
                "output" => cur_dir = "output",
                "inout" => cur_dir = "inout",
                "ref" => cur_dir = "ref",
                _ => {}
            }
        }
        if cur_dir != "output" {
            continue;
        }
        for dec in c.tree.children(p).filter(|&k| c.kind(k) == Declarator) {
            // Skip output port with a default value? No, output port defaults are caller-side defaults when omitted, body still computes output!
            if let Some(nm) = c.tree.child(dec, Name) {
                if let Some(nt) = c.first_tok(nm) {
                    let aname = c.tree.bare_text(nm);
                    if !aname.is_empty() {
                        outputs.push((aname, c.loc(nt)));
                    }
                }
            }
        }
    }
    if outputs.is_empty() {
        return;
    }
    let mut written: HashSet<String> = HashSet::new();
    let tracked: HashSet<String> = outputs.iter().map(|(n, _)| n.clone()).collect();
    mark_all_written_in_subtree(c, body, &tracked, &mut written);
    // Also check ConcatExpr LHS `{a, b} = ...`!
    for n in crate::support::slang_ast::subtree(c, body) {
        if c.kind(n) == BinaryExpr && is_any_assign_op(c, n) {
            if let Some(&lhs) = kids(c, n).first() {
                if c.kind(lhs) == ConcatExpr {
                    for sub in crate::support::slang_ast::subtree(c, lhs) {
                        if c.kind(sub) == NameExpr {
                            if let Some(t) = c.first_tok(sub) {
                                written.insert(c.text_of(t).to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    for (aname, loc) in outputs {
        if !written.contains(&aname) {
            out.push(UnwrittenOutput { routine_name: rname.to_string(), arg_name: aname, loc });
        }
    }
}

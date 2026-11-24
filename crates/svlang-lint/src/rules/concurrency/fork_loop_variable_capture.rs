//! SVL-CON-008 (ForkLoopVariableCapture): Disallows referencing loop variables
//! directly inside 'fork ... join_none' or 'join_any' blocks without an
//! automatic local variable copy. The copy may sit in the fork's own
//! declarations, which the parent thread initializes. The C++ rule is the same.

use crate::support::for_loops::{for_parts, for_vars, is_ident_name, keyword_is, kids, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

/// The names slang reads as `IdentifierName`s under `n`, in order, with
/// the names `DataDeclaration`s declared before each: plain names, member
/// names and user type names, but not the name before a select.
fn ident_names(c: &Cst, n: NodeId, locals: &mut HashSet<String>, out: &mut HashSet<String>) {
    let select_base =
        |x: NodeId| c.parent(x).is_some_and(|p| c.kind(p) == SelectExpr && kids(c, p).first() == Some(&x));
    match c.kind(n) {
        DataDecl => {
            for d in kids(c, n).into_iter().filter(|&d| c.kind(d) == Declarator) {
                if let Some(nm) = c.tree.child(d, Name) {
                    locals.insert(c.tree.bare_text(nm));
                }
            }
        }
        NameExpr if is_ident_name(c, n) && !select_base(n) => {
            let name = c.tree.bare_text(n);
            if !locals.contains(&name) {
                out.insert(name);
            }
        }
        MemberExpr if !select_base(n) => {
            if let Some((_, b)) = c.tree.token_bounds(n) {
                let name = c.text_of(b).to_string();
                if !locals.contains(&name) {
                    out.insert(name);
                }
            }
        }
        DataType => {
            if let Some((a, b)) = c.tree.token_bounds(n) {
                let t = c.text_of(a);
                if a == b && t.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') && !locals.contains(t) {
                    out.insert(t.to_string());
                }
            }
        }
        _ => {}
    }
    for k in c.tree.children(n) {
        ident_names(c, k, locals, out);
    }
}

pub(crate) fn fork_loop_capture(c: &Cst, s: &mut Sink) {
    let mut loops = Vec::new();
    for l in c.nodes(LoopStmt) {
        let mut decl_types: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let vars: Vec<String> = if keyword_is(c, l, "for") {
            let (init, _, _) = for_parts(c, l);
            for &it in &init {
                if c.kind(it) == DataDecl {
                    if let Some(dt) = c.tree.child(it, DataType) {
                        let dt_txt = c.trimmed(dt);
                        for d in kids(c, it).into_iter().filter(|&d| c.kind(d) == Declarator) {
                            if c.tree.child(d, Dim).is_none() {
                                if let Some(nm) = c.tree.child(d, Name) {
                                    decl_types.insert(c.tree.bare_text(nm), dt_txt.clone());
                                }
                            }
                        }
                    }
                }
            }
            for_vars(c, &init, true)
        } else if keyword_is(c, l, "foreach") {
            let toks: Vec<u32> = c.toks(l).collect();
            let open = toks.iter().position(|&t| c.text_of(t) == "[");
            open.map(|o| {
                toks[o..]
                    .iter()
                    .map(|&t| c.text_of(t))
                    .filter(|t| t.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_'))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
        } else {
            continue;
        };
        if let (false, Some(&body)) = (vars.is_empty(), kids(c, l).last()) {
            loops.push((vars, decl_types, body));
        }
    }
    for (vars, decl_types, body) in loops {
        let mut all = Vec::new();
        subtree(c, body, &mut all);
        for b in all.into_iter().filter(|&b| c.kind(b) == ParBlock) {
            let join = c.last_text(b);
            if join != "join_none" && join != "join_any" {
                continue;
            }
            let mut ids = HashSet::new();
            let mut locals = HashSet::new();
            for item in kids(c, b) {
                // The fork's own declarations are initialized by the parent
                // thread on entry, before any process is spawned (IEEE 1800
                // § 9.3.2): `automatic int k = i;` there is the copy the rule
                // asks for, not a read from a child thread.
                if c.kind(item) == DataDecl {
                    for d in kids(c, item).into_iter().filter(|&d| c.kind(d) == Declarator) {
                        if let Some(nm) = c.tree.child(d, Name) {
                            locals.insert(c.tree.bare_text(nm));
                        }
                    }
                    continue;
                }
                ident_names(c, item, &mut locals, &mut ids);
            }
            // The C++ rule walks a std::unordered_set, which for a few names in
            // distinct buckets yields the last inserted first.
            let Some(var) = vars.iter().rev().find(|v| ids.contains(*v)) else { continue };
            let Some(t) = c.first_tok(b) else { continue };
            let fixes =
                decl_types.get(var).map(|dt| automatic_copy_fixes(c, b, var, dt, &locals, &ids)).unwrap_or_default();
            s.add(
                "SVL-CON-008",
                "ForkLoopVariableCapture",
                Severity::Error,
                format!(
                    "Loop variable '{var}' is referenced directly inside a 'fork ... {join}' child thread without an isolating local variable copy. \
                     In SystemVerilog (IEEE 1800 § 9.3.2), threads spawned \
                     with join_none or join_any execute after the loop advances, causing all spawned child threads to read \
                     the final mutated value of the loop variable. Declare an automatic local variable copy \
                     (e.g. 'automatic int _{var} = {var};') inside the fork block to bind the per-iteration value."
                ),
                c.loc(t),
            )
            .fixes
            .extend(fixes);
        }
    }
}

fn automatic_copy_fixes(
    c: &Cst,
    par_block: NodeId,
    var: &str,
    dt_text: &str,
    locals: &HashSet<String>,
    ids: &HashSet<String>,
) -> Vec<crate::fix::Fix> {
    let copy_name = format!("__{var}");
    if locals.contains(&copy_name) || ids.contains(&copy_name) {
        return Vec::new();
    }
    let Some(fork_tok) = c.tok(par_block, "fork") else { return Vec::new() };
    let fk = &c.tree.tokens[fork_tok as usize];
    if fk.from_macro {
        return Vec::new();
    }
    let file = fk.origin.file;
    // If `fork : block_name` has a header `Name` child, insert after that `Name`.
    let mut insert_after_tok = fork_tok;
    if let Some(k) = c.tree.children(par_block).next().filter(|&k| c.kind(k) == Name) {
        if let Some((_, end_t)) = c.tree.token_bounds(k) {
            insert_after_tok = end_t;
        }
    }
    let ins_k = &c.tree.tokens[insert_after_tok as usize];
    if ins_k.from_macro || ins_k.origin.file != file {
        return Vec::new();
    }
    let insert_at = ins_k.origin.offset as usize + ins_k.text.len();

    // Refuse if `var` is mutated (`=`, `+=`, `++`, `--`) inside `par_block`.
    let mut sub = Vec::new();
    subtree(c, par_block, &mut sub);
    for &n in &sub {
        match c.kind(n) {
            BinaryExpr
                if c.toks(n).any(|t| {
                    c.text_of(t).ends_with('=')
                        && !matches!(c.text_of(t), "==" | "!=" | "===" | "!==" | "<=" | ">=" | "==?" | "!=?")
                }) =>
            {
                if let Some(lhs) = c.tree.children(n).next() {
                    if c.tree.bare_text(lhs) == var {
                        return Vec::new();
                    }
                }
            }
            PrefixExpr | PostfixExpr
                if c.toks(n).any(|t| matches!(c.text_of(t), "++" | "--")) && c.tree.bare_text(n).contains(var) =>
            {
                return Vec::new();
            }
            _ => {}
        }
    }

    // Collect standalone token occurrences of `var` inside non-DataDecl items of `par_block`.
    let mut ref_toks = Vec::new();
    for item in kids(c, par_block) {
        if matches!(c.kind(item), DataDecl | Name | EndLabel) {
            continue;
        }
        for tid in c.tree.token_range(item) {
            if c.text_of(tid) != var {
                continue;
            }
            let prev = tid.checked_sub(1).map(|p| c.text_of(p));
            if matches!(prev, Some("." | "::")) {
                continue;
            }
            let tk = &c.tree.tokens[tid as usize];
            if tk.from_macro || tk.origin.file != file || (tk.origin.offset as usize) <= insert_at {
                return Vec::new();
            }
            ref_toks.push(tid);
        }
    }
    if ref_toks.is_empty() {
        return Vec::new();
    }

    let Some(src) = c.tree.sources.get(file as usize).map(|s| s.as_bytes()) else { return Vec::new() };
    let mut ls = fk.origin.offset as usize;
    while ls > 0 && src[ls - 1] != b'\n' {
        ls -= 1;
    }
    let mut le = ls;
    while le < src.len() && matches!(src[le], b' ' | b'\t') {
        le += 1;
    }
    let base_indent = String::from_utf8_lossy(&src[ls..le]);
    let indent = format!("{base_indent}    ");

    let mut out = Vec::with_capacity(1 + ref_toks.len());
    if let Some(ins) = crate::fix::with_safety(
        Some(c.file_fix(
            file,
            insert_at,
            insert_at,
            format!("\n{indent}automatic {dt_text} {copy_name} = {var};"),
            &format!("Bind per-iteration loop variable '{var}' in an automatic local copy"),
        )),
        crate::fix::FixSafety::Unsafe,
    ) {
        out.push(ins);
    }
    for tid in ref_toks {
        let tk = &c.tree.tokens[tid as usize];
        let start = tk.origin.offset as usize;
        let end = start + tk.text.len();
        if let Some(rep) = crate::fix::with_safety(
            Some(c.file_fix(
                file,
                start,
                end,
                copy_name.clone(),
                &format!("Use per-iteration automatic copy '{copy_name}'"),
            )),
            crate::fix::FixSafety::Unsafe,
        ) {
            out.push(rep);
        }
    }
    out
}

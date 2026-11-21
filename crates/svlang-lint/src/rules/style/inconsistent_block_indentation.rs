//! SV-STY-020 (InconsistentBlockIndentation): Uses [`crate::shared::tree_layout`]
//! to flag multi-line `begin ... end`, `class`, `module`, `function`, and `task`
//! blocks whose direct child items/statements are unindented relative to the
//! block header or inconsistently indented relative to sibling items, offering
//! safe whitespace `--fix` rewrites.

use crate::support::source_layout::at;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn block_children(c: &Cst, n: NodeId) -> Vec<NodeId> {
    match c.kind(n) {
        FunctionDecl | TaskDecl => c.tree.child(n, Body).map(|b| c.tree.children(b).collect()).unwrap_or_default(),
        SeqBlock => c.tree.children(n).filter(|&k| c.kind(k) != Name && c.kind(k) != EndLabel).collect(),
        ClassDecl | ModuleDecl | InterfaceDecl => c
            .tree
            .children(n)
            .filter(|&k| !matches!(c.kind(k), Name | Header | Extends | EndLabel | Attribute))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn inconsistent_block_indentation(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let indent_step = crate::options::get("SV-STY-020", "indent_spaces", 2).max(1) as u32;

    for n in c.tree.preorder() {
        if !matches!(c.kind(n), SeqBlock | ClassDecl | ModuleDecl | InterfaceDecl | FunctionDecl | TaskDecl) {
            continue;
        }
        let Some((first_t, last_t)) = c.tree.token_bounds(n) else { continue };
        if is_library_tok(c, first_t) || is_library_tok(c, last_t) {
            continue;
        }
        let Some((f0, open_line, _)) = layout.source_token_pos(c.tree, first_t) else { continue };
        let Some((f1, close_line, _)) = layout.source_token_pos(c.tree, last_t) else { continue };
        if f0 != f1 || !c.tree.mains.contains(&f0) || close_line <= open_line + 1 {
            continue;
        }
        // For `if (...) begin` on the same line as `if`, use the `open_line`'s indent as the header indent.
        let Some(fl) = layout.file(f0) else { continue };
        let Some(open_ll) = fl.line(open_line) else { continue };
        if open_ll.in_define_or_skipped {
            continue;
        }
        let header_indent = open_ll.indent_cols;
        let kids = block_children(c, n);
        if kids.is_empty() {
            continue;
        }

        // Gather `(child_first_tok, line_1based, indent_cols)` for direct children that
        // start their own line inside `(open_line, close_line)`.
        let mut rows: Vec<(u32, u32, u32)> = Vec::new();
        let mut prev_line = open_line;
        for k in kids {
            let Some(kt) = c.first_tok(k) else { continue };
            let Some((kf, kline, _)) = layout.source_token_pos(c.tree, kt) else { continue };
            if kf != f0 || kline <= prev_line || kline >= close_line {
                continue;
            }
            let Some(kll) = fl.line(kline) else { continue };
            if kll.in_define_or_skipped || kll.has_leading_tab || kll.first_token != Some(kt) {
                continue;
            }
            rows.push((kt, kline, kll.indent_cols));
            prev_line = kline;
        }
        if rows.is_empty() {
            continue;
        }

        // Expected indent: if the first child is properly indented (`> header_indent`), adopt its
        // indent when >= 2 children exist, else `header_indent + indent_step`.
        let expected =
            rows.iter().map(|&(_, _, ind)| ind).find(|&ind| ind > header_indent).unwrap_or(header_indent + indent_step);

        for (kt, kline, ind) in rows {
            if ind == expected {
                continue;
            }
            // Flag if unindented (`ind <= header_indent`) or misaligned with siblings (`ind != expected`).
            let Some(kll) = fl.line(kline) else { continue };
            let tok_off = c.tree.tokens[kt as usize].origin.offset as usize;
            let replacement = " ".repeat(expected as usize);
            out.add(
                "SV-STY-020",
                "InconsistentBlockIndentation",
                Severity::Info,
                format!(
                    "Statement or item on line {kline} is indented by {ind} column(s), expected {expected} column(s) inside block opened at line {open_line}."
                ),
                at(c, f0, tok_off),
            )
            .fixes
            .push(c.file_fix(f0, kll.start as usize, tok_off, replacement, "Normalize block indentation"));
        }
    }
}

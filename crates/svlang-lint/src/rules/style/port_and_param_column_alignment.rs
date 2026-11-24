//! SVL-STY-019 (PortAndParamColumnAlignment): Uses [`crate::shared::tree_layout`]
//! to check multi-line ANSI module/interface port and parameter lists that
//! already use tabular column alignment (`>= 2/3` of items aligned at the same
//! identifier column with multi-space padding), flagging items whose declared
//! identifier drifted out of column alignment and offering a safe whitespace
//! `--fix` rewrite.

use std::collections::HashMap;

use crate::support::source_layout::at;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn check_list(c: &Cst, layout: &crate::support::layout::TreeLayout, list: NodeId, out: &mut Sink) {
    let items: Vec<NodeId> = c.tree.children(list).filter(|&k| matches!(c.kind(k), Port | ParamDecl)).collect();
    if items.len() < 3 {
        return;
    }
    let Some((first_t, last_t)) = c.tree.token_bounds(list) else { return };
    if is_library_tok(c, first_t) || is_library_tok(c, last_t) {
        return;
    }
    let Some((f0, open_l, _)) = layout.source_token_pos(c.tree, first_t) else { return };
    let Some((f1, close_l, _)) = layout.source_token_pos(c.tree, last_t) else { return };
    if f0 != f1 || !c.tree.mains.contains(&f0) || close_l <= open_l + 2 {
        return;
    }
    let Some(fl) = layout.file(f0) else { return };
    let Some(src) = c.tree.sources.get(f0 as usize).map(|s| s.as_bytes()) else { return };

    // Collect `(name_tok, line_1based, name_col_1based, min_col_after_prev_tok, pad_spaces)`
    // for items that occupy their own source line without tabs or macros.
    struct Row {
        name_tok: u32,
        line: u32,
        col: u32,
        prev_end_off: usize,
        name_off: usize,
        min_col: u32,
        padded: bool,
    }
    let mut rows: Vec<Row> = Vec::with_capacity(items.len());
    let mut prev_line = 0u32;
    for item in items {
        let decls: Vec<NodeId> = c.tree.children(item).filter(|&d| c.kind(d) == Declarator).collect();
        let [decl] = decls.as_slice() else { return };
        let Some(nm) = c.tree.child(*decl, Name) else { return };
        let Some(nt) = c.first_tok(nm) else { return };
        if nt == 0 {
            return;
        }
        let Some((nf, nline, ncol)) = layout.source_token_pos(c.tree, nt) else { return };
        if nf != f0 || nline <= prev_line {
            return;
        }
        let Some(ll) = fl.line(nline) else { return };
        if ll.in_define_or_skipped || ll.has_leading_tab {
            return;
        }
        let prev_tok = nt - 1;
        let Some((pf, pline, _)) = layout.source_token_pos(c.tree, prev_tok) else { return };
        if pf != f0 || pline != nline {
            return;
        }
        let pk = &c.tree.tokens[prev_tok as usize];
        let nk = &c.tree.tokens[nt as usize];
        let prev_end_off = pk.origin.offset as usize + pk.text.len();
        let name_off = nk.origin.offset as usize;
        if prev_end_off > name_off || name_off > src.len() {
            return;
        }
        // Only consider rows with plain ASCII spaces between the type/range and identifier.
        if src[prev_end_off..name_off].iter().any(|&b| b != b' ') {
            return;
        }
        let gap = (name_off - prev_end_off) as u32;
        let min_col = ncol.saturating_sub(gap) + 1;
        rows.push(Row { name_tok: nt, line: nline, col: ncol, prev_end_off, name_off, min_col, padded: gap >= 2 });
        prev_line = nline;
    }

    if rows.len() < 3 {
        return;
    }

    let mut col_counts: HashMap<u32, usize> = HashMap::new();
    for r in &rows {
        *col_counts.entry(r.col).or_default() += 1;
    }
    let Some((&target_col, &aligned_n)) = col_counts.iter().max_by_key(|&(_, cnt)| cnt) else {
        return;
    };
    // Require a clear majority (`aligned_n * 3 >= rows.len() * 2` and `aligned_n >= 2`) AND
    // at least one aligned row using `>= 2` padding spaces (confirming deliberate tabular alignment).
    let intentional_table =
        aligned_n >= 2 && aligned_n * 3 >= rows.len() * 2 && rows.iter().any(|r| r.col == target_col && r.padded);
    if !intentional_table {
        return;
    }

    for r in &rows {
        if r.col == target_col {
            continue;
        }
        let ident = c.text_of(r.name_tok);
        let finding = out.add(
            "SVL-STY-019",
            "PortAndParamColumnAlignment",
            Severity::Info,
            format!(
                "Identifier '{ident}' on line {} starts at column {}, breaking tabular column {} alignment of surrounding declarations.",
                r.line, r.col, target_col
            ),
            at(c, f0, r.name_off),
        );
        if r.min_col <= target_col {
            let spaces = (target_col - r.min_col + 1) as usize;
            finding.fixes.push(c.file_fix(
                f0,
                r.prev_end_off,
                r.name_off,
                " ".repeat(spaces),
                "Align declaration identifier to tabular column",
            ));
        }
    }
}

pub(crate) fn port_and_param_column_alignment(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    for hdr in c.nodes(Header) {
        if let Some(pl) = c.tree.child(hdr, ParamList) {
            check_list(c, &layout, pl, out);
        }
        if let Some(pl) = c.tree.child(hdr, PortList) {
            check_list(c, &layout, pl, out);
        }
    }
}

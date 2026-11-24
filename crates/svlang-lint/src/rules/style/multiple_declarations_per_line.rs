//! SVL-STY-015 (MultipleDeclarationsPerLine): Uses [`crate::shared::tree_layout`]
//! to flag multiple variable/property declarations placed on the same source
//! line inside multi-line blocks, classes, or modules, offering a safe `--fix`
//! newline split for back-to-back declarations (`int a; int b;`).

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

pub(crate) fn multiple_declarations_per_line(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let allow_comma = crate::options::get("SVL-STY-015", "allow_comma_list", 0) != 0;

    for container in c.tree.preorder() {
        if !matches!(c.kind(container), ClassDecl | ModuleDecl | InterfaceDecl | SeqBlock | Body) {
            continue;
        }
        let Some((ct0, ct1)) = c.tree.token_bounds(container) else { continue };
        let Some((cf0, cline0, _)) = layout.source_token_pos(c.tree, ct0) else { continue };
        let Some((cf1, cline1, _)) = layout.source_token_pos(c.tree, ct1) else { continue };
        // Skip single-line compact containers (e.g., `module m; int a; int b; endmodule`).
        if cf0 != cf1 || !c.tree.mains.contains(&cf0) || cline0 == cline1 {
            continue;
        }
        let Some(fl) = layout.file(cf0) else { continue };

        let mut prev_decl_on_line: Option<(u32, NodeId)> = None;
        for child in c.tree.children(container).filter(|&k| c.kind(k) == DataDecl) {
            let Some((dt0, _)) = c.tree.token_bounds(child) else { continue };
            if is_library_tok(c, dt0) {
                continue;
            }
            let Some((df, dline, _)) = layout.source_token_pos(c.tree, dt0) else { continue };
            if df != cf0 || dline == cline0 || dline == cline1 {
                continue;
            }
            let Some(ll) = fl.line(dline) else { continue };
            if ll.in_define_or_skipped {
                continue;
            }

            if let Some((pline, prev_node)) = prev_decl_on_line {
                if pline == dline {
                    let fix = c.tree.token_bounds(prev_node).and_then(|(_, p_end)| {
                        let p_tok = &c.tree.tokens[p_end as usize];
                        let d_tok = &c.tree.tokens[dt0 as usize];
                        if !p_tok.from_macro && !d_tok.from_macro && p_tok.origin.file == cf0 {
                            let after_prev = p_tok.origin.offset as usize + p_tok.text.len();
                            let before_cur = d_tok.origin.offset as usize;
                            let indent = " ".repeat(ll.indent_cols as usize);
                            Some(c.file_fix(
                                cf0,
                                after_prev,
                                before_cur,
                                format!("\n{indent}"),
                                "Place second declaration on its own line",
                            ))
                        } else {
                            None
                        }
                    });
                    out.add(
                        "SVL-STY-015",
                        "MultipleDeclarationsPerLine",
                        Severity::Info,
                        "Multiple variable or property declarations on the same line. Place each declaration on its own line for readable diffs and wave inspection."
                            .into(),
                        c.loc(dt0),
                    )
                    .fixes
                    .extend(fix);
                    prev_decl_on_line = Some((dline, child));
                    continue;
                }
            }
            prev_decl_on_line = Some((dline, child));

            if !allow_comma {
                let decls: Vec<NodeId> = c.tree.children(child).filter(|&x| c.kind(x) == Declarator).collect();
                if decls.len() > 1 {
                    if let Some(second_tok) = c.first_tok(decls[1]) {
                        out.add(
                            "SVL-STY-015",
                            "MultipleDeclarationsPerLine",
                            Severity::Info,
                            format!(
                                "Declaration groups {} variables in a comma-separated list on one line. Declare one variable or property per line.",
                                decls.len()
                            ),
                            c.loc(second_tok),
                        );
                    }
                }
            }
        }
    }
}

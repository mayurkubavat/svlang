//! SV-STY-022 (UnparenthesizedMacroArg): Uses the Preprocessor Event Table
//! ([`svlang::syntax::Tree::macro_defs`]) to flag function-like expression
//! macros whose formal parameters appear adjacent to binary or unary operators
//! in the replacement body without protective `(param)` parentheses, offering a
//! safe `--fix` rewrite.

use std::collections::HashSet;

use crate::support::source_layout::at;
use crate::support::trivia::library_file;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::lexer::Kind;

const EXPR_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "<<", ">>", "<<<", ">>>", "&", "|", "^", "~^", "^~", "&&", "||", "<", "<=", ">",
    ">=", "==", "!=", "===", "!==", "==?", "!=?", "?", ":", "~", "!",
];

const STMT_KEYWORDS: &[&str] = &[
    "begin",
    "end",
    "if",
    "else",
    "for",
    "foreach",
    "while",
    "repeat",
    "forever",
    "do",
    "case",
    "casex",
    "casez",
    "endcase",
    "class",
    "endclass",
    "module",
    "endmodule",
    "function",
    "endfunction",
    "task",
    "endtask",
    "return",
    "assign",
    "always",
    "always_comb",
    "always_ff",
    "initial",
    "typedef",
    "fork",
    "join",
    "join_none",
    "join_any",
];

pub(crate) fn unparenthesized_macro_arg(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    for def in layout.main_macro_defs(c.tree) {
        if library_file(c, def.origin.file) || def.name.starts_with("uvm_") || def.name.starts_with("UVM_") {
            continue;
        }
        let Some(params) = &def.params else { continue };
        if params.is_empty() || def.body.is_empty() {
            continue;
        }
        let param_set: HashSet<&str> = params.iter().map(|p| &**p).collect();

        // Restrict to expression-like macro bodies (no `;` and no statement/declaration keywords).
        let is_stmt_macro = def.body.iter().any(|(_, txt, _)| {
            let s = &**txt;
            s == ";" || STMT_KEYWORDS.contains(&s) || s.starts_with("`uvm_")
        });
        if is_stmt_macro {
            continue;
        }

        let mut reported = HashSet::new();
        for (i, (kind, txt, orig)) in def.body.iter().enumerate() {
            if *kind != Kind::Identifier || !param_set.contains(&**txt) {
                continue;
            }
            let prev = i.checked_sub(1).map(|k| &*def.body[k].1);
            let next = def.body.get(i + 1).map(|x| &*x.1);
            // Skip token-pasting (``), stringification (`"), member `.param`, or already `(param)`.
            if prev.is_some_and(|p| p == "``" || p == "`\"" || p == "." || p == "::")
                || next.is_some_and(|n| n == "``" || n == "`\"")
            {
                continue;
            }
            if prev == Some("(") && next == Some(")") {
                continue;
            }
            let prev_is_op = prev.is_some_and(|p| EXPR_OPS.contains(&p));
            let next_is_op = next.is_some_and(|n| EXPR_OPS.contains(&n));
            if (prev_is_op || next_is_op) && reported.insert(&**txt) {
                let off = orig.offset as usize;
                let end = off + txt.len();
                out.add(
                    "SV-STY-022",
                    "UnparenthesizedMacroArg",
                    Severity::Warning,
                    format!(
                        "Macro parameter '{txt}' in expression macro '`{}' is adjacent to an operator without enclosing parentheses. Wrap it as '({txt})' to prevent operator-precedence bugs at call sites.",
                        def.name
                    ),
                    at(c, orig.file, off),
                )
                .fixes
                .push(c.file_fix(
                    orig.file,
                    off,
                    end,
                    format!("({txt})"),
                    "Parenthesize macro parameter in replacement body",
                ));
            }
        }
    }
}

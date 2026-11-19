//! SV-WVR-001 (UnclosedBlockWaiver): Scoped block waiver (`uvmcheck:off`,
//! `verissimo_lint: waive_start`, `lint:waive-start`) never closed before EOF.

use dv_model::Loc;

use crate::support::defines_packages::{user_tok, Files};
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unclosed_block_waiver(c: &Cst, out: &mut Sink) {
    let fs = Files::new(c);
    let mut checked_files = std::collections::HashSet::new();
    for (tok_idx, tok) in c.tree.tokens.iter().enumerate() {
        if tok.from_macro {
            continue;
        }
        let file_ix = tok.origin.file;
        if !checked_files.insert(file_ix) {
            continue;
        }
        if !user_tok(c, &fs, tok_idx as u32) {
            continue;
        }
        let Some(src) = c.tree.sources.get(file_ix as usize) else { continue };
        let unclosed = crate::waivers::find_unclosed_blocks(src);
        if unclosed.is_empty() {
            continue;
        }
        let base_loc = c.loc(tok_idx as u32);
        for (line, rules) in unclosed {
            out.add(
                "SV-WVR-001",
                "UnclosedBlockWaiver",
                Severity::Warning,
                format!(
                    "Scoped block waiver for '{}' opened on line {} is never closed with a matching 'uvmcheck:on' or 'verissimo_lint: waive_stop' directive, silently suppressing diagnostics through the end of the file.",
                    rules, line
                ),
                Loc { file: base_loc.file, line, column: 1, expansion: None },
            );
        }
    }
}

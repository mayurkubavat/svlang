//! SVL-STY-017 (UnusedIncludeDirective): Uses the Preprocessor Event Table
//! ([`svlang::syntax::Tree::includes`], [`svlang::syntax::Tree::macro_defs`],
//! [`svlang::syntax::Tree::macro_uses`]) to flag `` `include `` directives in
//! top-level files (outside `package` aggregation bodies) whose included header
//! contributes neither parsed tokens nor used macros.

use std::collections::HashSet;

use crate::support::source_layout::at;
use crate::support::trivia::library_file;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;

pub(crate) fn unused_include_directive(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let tree = c.tree;

    // Ranges of `package ... endpackage` in main files: package bodies routinely
    // `include` component/sequence headers to export them to importers.
    let mut pkg_ranges: Vec<(u32, u32, u32)> = Vec::new();
    for p in c.nodes(PackageDecl) {
        if let Some((a, b)) = tree.token_bounds(p) {
            let ta = &tree.tokens[a as usize];
            let tb = &tree.tokens[b as usize];
            if !ta.from_macro && !tb.from_macro && ta.origin.file == tb.origin.file {
                pkg_ranges.push((ta.origin.file, ta.origin.offset, tb.origin.offset + tb.text.len() as u32));
            }
        }
    }

    let used_macro_names: HashSet<&str> = tree.macro_uses.iter().map(|u| &*u.name).collect();

    for edge in layout.main_includes(tree) {
        if library_file(c, edge.origin.file) {
            continue;
        }
        let raw = &*edge.raw_name;
        if raw.contains("uvm_macros") || raw.contains("uvm_pkg") {
            continue;
        }
        // Skip includes written inside a `package ... endpackage` body.
        if pkg_ranges
            .iter()
            .any(|&(pf, pa, pb)| pf == edge.origin.file && edge.origin.offset >= pa && edge.origin.offset <= pb)
        {
            continue;
        }

        // Transitive included file set starting from `edge.target_file`.
        let mut target_files: HashSet<u32> = HashSet::new();
        let mut stack = vec![edge.target_file];
        while let Some(f) = stack.pop() {
            if target_files.insert(f) {
                for sub in tree.includes.iter().filter(|x| x.origin.file == f) {
                    stack.push(sub.target_file);
                }
            }
        }

        // Did `target_files` contribute any parsed token in `tree.tokens`?
        let contributed_tokens = tree
            .tokens
            .iter()
            .any(|tok| target_files.contains(&tok.origin.file) || target_files.contains(&tok.written.file));
        if contributed_tokens {
            continue;
        }

        // Did `target_files` define any macro that was referenced in `macro_uses`
        // or tested in a conditional directive in the main file?
        let main_src = &tree.sources[edge.origin.file as usize];
        let contributed_macros = tree.macro_defs.iter().any(|d| {
            target_files.contains(&d.origin.file)
                && (used_macro_names.contains(&*d.name)
                    || (main_src.contains("`ifdef") && main_src.contains(&*d.name))
                    || (main_src.contains("`ifndef") && main_src.contains(&*d.name)))
        });
        if contributed_macros {
            continue;
        }

        // Skip empty stub files.
        if tree.sources.get(edge.target_file as usize).is_some_and(|s| s.trim().is_empty()) {
            continue;
        }

        out.add(
            "SVL-STY-017",
            "UnusedIncludeDirective",
            Severity::Info,
            format!(
                "Included file '{raw}' contributes no declarations, tokens, or used macros to this compilation unit. Remove unused '`include' directives to speed up preprocessing."
            ),
            at(c, edge.origin.file, edge.origin.offset as usize),
        );
    }
}

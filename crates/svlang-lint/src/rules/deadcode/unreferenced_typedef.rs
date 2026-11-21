//! SV-DED-016 (UnreferencedTypedef): Flags `typedef` declarations inside
//! modules, interfaces, classes, tasks, and functions (excluding `package`
//! public APIs and UVM `type_id` / `this_type` idioms) whose defined type
//! identifier never appears anywhere else in the enclosing scope or compile.

use std::collections::HashSet;

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;

pub(crate) fn unreferenced_typedef(c: &Cst, out: &mut Sink) {
    // Collect all identifier token texts across all trees in the compile, counting
    // occurrences so a single occurrence (the `TypedefDecl` name itself) is O(1) to spot.
    let mut global_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let trees: Vec<&svlang::syntax::Tree> = if c.all.is_empty() { vec![c.tree] } else { c.all.iter().collect() };
    for tr in &trees {
        for tok in &tr.tokens {
            if tok.kind == svlang::lexer::Kind::Identifier {
                *global_counts.entry(&tok.text).or_default() += 1;
            }
        }
    }

    let mut seen = HashSet::new();
    for td in c.nodes(TypedefDecl) {
        let Some(nm) = c.tree.child(td, Name) else { continue };
        let Some(nt) = c.first_tok(nm) else { continue };
        if is_library_tok(c, nt) {
            continue;
        }
        let tname = c.tree.bare_text(nm);
        if matches!(tname.as_str(), "type_id" | "this_type" | "type_name") {
            continue;
        }
        // Only check typedefs scoped inside a module, interface, program, class, function, or task
        // (top-level `$unit` and `package` typedefs may be exported to downstream units).
        let Some(enclosing) = std::iter::successors(c.parent(td), |&a| c.parent(a)).find(|&a| {
            matches!(
                c.kind(a),
                PackageDecl | ModuleDecl | InterfaceDecl | ProgramDecl | ClassDecl | FunctionDecl | TaskDecl
            )
        }) else {
            continue;
        };
        if c.kind(enclosing) == PackageDecl {
            continue;
        }
        // Count non-declaration identifier occurrences: if total occurrences across the compile == 1
        // (the `TypedefDecl` `Name` token itself), or inside a local routine/module where no other
        // token inside `enclosing` (for non-class scopes) / compile (for class scopes, which can be
        // referenced via `cls::t`) has text `tname`, it is unreferenced.
        let total = global_counts.get(tname.as_str()).copied().unwrap_or(0);
        let used = if c.kind(enclosing) == ClassDecl {
            total > 1
        } else {
            c.tree.token_range(enclosing).into_iter().any(|t| t != nt && c.text_of(t) == tname)
        };
        if !used && seen.insert((c.tree.tokens[nt as usize].origin.file, c.tree.tokens[nt as usize].origin.offset)) {
            out.add(
                "SV-DED-016",
                "UnreferencedTypedef",
                Severity::Info,
                format!(
                    "Typedef '{tname}' is declared but never referenced in its scope. Remove dead type aliases to keep declarations clean."
                ),
                c.loc(nt),
            );
        }
    }
}

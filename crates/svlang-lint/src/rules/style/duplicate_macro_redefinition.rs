//! SVL-MNT-056 (DuplicateMacroRedefinition): Uses the Preprocessor Event Table
//! ([`svlang::syntax::Tree::macro_defs`] and [`svlang::syntax::Tree::cond_events`])
//! to flag silently redefining an already-defined user macro in the same file
//! with a different replacement body without an intervening `` `undef `` or
//! mutually exclusive conditional compilation guard (IEEE 1800 §22.5.1).

use std::collections::HashMap;

use crate::support::source_layout::at;
use crate::support::trivia::library_file;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::CondKind;

fn normalized_body(def: &svlang::syntax::MacroDef) -> Vec<(&str, &str)> {
    let params =
        def.params.as_ref().map(|ps| ps.iter().map(|p| ("param", &**p)).collect::<Vec<_>>()).unwrap_or_default();
    let mut out = params;
    out.extend(def.body.iter().map(|(_, t, _)| ("tok", &**t)));
    out
}

fn has_undef_between(src: &str, from_off: u32, to_off: u32, name: &str) -> bool {
    let a = (from_off as usize).min(src.len());
    let b = (to_off as usize).min(src.len());
    if a >= b {
        return false;
    }
    for line in src[a..b].lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("`undef") {
            let sym = rest.split_whitespace().next().unwrap_or("");
            if sym == name {
                return true;
            }
        } else if trimmed.starts_with("`undefineall") {
            return true;
        }
    }
    false
}

/// Returns the nesting signature of active conditional compilation branches at `(file, offset)`.
fn cond_branch_id(c: &Cst, file: u32, offset: u32) -> Vec<(u32, u32)> {
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for ev in &c.tree.conditionals {
        if ev.origin.file != file || ev.origin.offset > offset {
            continue;
        }
        match ev.kind {
            CondKind::Ifdef | CondKind::Ifndef => {
                stack.push((ev.origin.offset, 0));
            }
            CondKind::Elsif | CondKind::Else => {
                if let Some(last) = stack.last_mut() {
                    last.1 += 1;
                }
            }
            CondKind::Endif => {
                stack.pop();
            }
        }
    }
    stack
}

type SeenMacro<'a> = (&'a svlang::syntax::MacroDef, Vec<(u32, u32)>);

pub(crate) fn duplicate_macro_redefinition(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let mut seen: HashMap<(u32, &str), SeenMacro<'_>> = HashMap::new();

    for def in layout.main_macro_defs(c.tree) {
        let file = def.origin.file;
        if library_file(c, file) {
            continue;
        }
        let name = &*def.name;
        let branch = cond_branch_id(c, file, def.origin.offset);
        if let Some(&(prev, ref prev_branch)) = seen.get(&(file, name)) {
            let src = c.tree.sources.get(file as usize).map(|s| &**s).unwrap_or("");
            if !has_undef_between(src, prev.origin.offset, def.origin.offset, name)
                && prev_branch == &branch
                && normalized_body(prev) != normalized_body(def)
            {
                let prev_loc = at(c, prev.origin.file, prev.origin.offset as usize);
                out.add(
                    "SVL-MNT-056",
                    "DuplicateMacroRedefinition",
                    Severity::Warning,
                    format!(
                        "Macro '`{name}' is redefined with a different replacement body without an intervening '`undef {name}' (previously defined on line {}). Silent macro overrides depend on compilation order per IEEE 1800 §22.5.1.",
                        prev_loc.line
                    ),
                    at(c, file, def.origin.offset as usize),
                );
            }
        }
        seen.insert((file, name), (def, branch));
    }
}

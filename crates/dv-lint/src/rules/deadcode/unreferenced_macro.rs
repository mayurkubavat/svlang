//! SV-DED-017 (UnreferencedMacro): Uses the Preprocessor Event Table
//! ([`svlang::syntax::Tree::macro_defs`] and [`svlang::syntax::Tree::cond_events`])
//! to flag file-local `` `define `` macros declared in `.sv` implementation
//! files (excluding `.svh`/`.vh` headers, include guards, and feature flags
//! queried by `` `ifdef ``/`` `ifndef ``) that are never expanded anywhere in
//! the compilation.

use std::collections::HashSet;

use crate::support::source_layout::at;
use crate::support::trivia::library_file;
use crate::syntax::Cst;
use crate::{Severity, Sink};

fn is_header_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".svh") || lower.ends_with(".vh") || lower.ends_with(".h")
}

fn is_guard_like(name: &str, body_empty: bool) -> bool {
    if body_empty {
        return true;
    }
    let up = name.to_ascii_uppercase();
    up.ends_with("_SV")
        || up.ends_with("_SVH")
        || up.ends_with("_VH")
        || up.ends_with("_H")
        || up.ends_with("_INCLUDED")
}

fn collect_backtick_uses<'a>(src: &'a str, out: &mut HashSet<&'a str>) {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip `//` and `/* ... */` comments so commented-out `` `FOO `` does not count as a use.
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start {
                let ident = &src[start..end];
                if !matches!(
                    ident,
                    "define"
                        | "undef"
                        | "undefineall"
                        | "ifdef"
                        | "ifndef"
                        | "elsif"
                        | "else"
                        | "endif"
                        | "include"
                        | "timescale"
                        | "default_nettype"
                        | "resetall"
                        | "line"
                        | "pragma"
                        | "begin_keywords"
                        | "end_keywords"
                        | "celldefine"
                        | "endcelldefine"
                ) {
                    out.insert(ident);
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
}

pub(crate) fn unreferenced_macro(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    let trees: Vec<&svlang::syntax::Tree> = if c.all.is_empty() { vec![c.tree] } else { c.all.iter().collect() };

    let mut used_macros: HashSet<String> = HashSet::new();
    for tr in &trees {
        for mu in &tr.macro_uses {
            used_macros.insert(mu.name.to_string());
        }
        for ev in &tr.conditionals {
            if let Some(src) = tr.sources.get(ev.origin.file as usize) {
                let off = ev.origin.offset as usize;
                if off < src.len() {
                    let line = src[off..].lines().next().unwrap_or("");
                    for part in line.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
                        if !part.is_empty() && !matches!(part, "ifdef" | "ifndef" | "elsif" | "else" | "endif") {
                            used_macros.insert(part.to_string());
                        }
                    }
                }
            }
        }
        for src in &tr.sources {
            let mut refs = HashSet::new();
            collect_backtick_uses(src, &mut refs);
            for r in refs {
                used_macros.insert(r.to_string());
            }
        }
    }

    for def in layout.main_macro_defs(c.tree) {
        let file = def.origin.file;
        let path_str = c.tree.files.get(file as usize).map(|p| p.to_string_lossy()).unwrap_or_default();
        let path = path_str.as_ref();
        if library_file(c, file) || is_header_path(path) {
            continue;
        }
        let name = &*def.name;
        if name.starts_with("uvm_") || name.starts_with("UVM_") || is_guard_like(name, def.body.is_empty()) {
            continue;
        }
        if !used_macros.contains(name) {
            out.add(
                "SV-DED-017",
                "UnreferencedMacro",
                Severity::Info,
                format!(
                    "Macro '`{name}' is defined in '{path}' but never expanded or queried in conditional compilation. Remove dead file-local macros or move shared definitions to a header."
                ),
                at(c, file, def.origin.offset as usize),
            );
        }
    }
}

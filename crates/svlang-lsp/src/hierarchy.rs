//! LSP Type Hierarchy (UVM & SV class inheritance) and Call Hierarchy
//! (functions, tasks, and class methods across the workspace).

use std::collections::HashSet;
use std::path::PathBuf;

use crate::index::SymKind;
use crate::json::{obj, Json};
use crate::server::symbol_kind;
use crate::text::{is_ident_byte, path_to_uri, uri_to_path};
use crate::workspace::{SymRef, Workspace};

fn range_json(ws: &Workspace, f: usize, a: u32, b: u32) -> Json {
    let file = ws.file(f);
    let (l0, c0) = file.position(a);
    let (l1, c1) = file.position(b.max(a));
    obj(vec![
        ("start", obj(vec![("line", l0.into()), ("character", c0.into())])),
        ("end", obj(vec![("line", l1.into()), ("character", c1.into())])),
    ])
}

fn bare_type(raw: &str) -> &str {
    let no_params = raw.split('#').next().unwrap_or(raw).trim();
    no_params.rsplit("::").next().unwrap_or(no_params).trim()
}

fn hierarchy_item(ws: &Workspace, r: SymRef) -> Json {
    let s = ws.sym(r);
    let file = ws.file(r.0);
    let in_class = s.parent.is_some_and(|p| file.symbols[p as usize].kind == SymKind::Class) || s.scope_class.is_some();
    let detail = if s.kind == SymKind::Class {
        s.type_name.as_ref().map(|b| format!("extends {b}")).unwrap_or_else(|| "class".into())
    } else if let Some(p) = s.parent {
        format!("{} {}", file.symbols[p as usize].kind.label(), file.symbols[p as usize].name)
    } else if let Some(c) = &s.scope_class {
        format!("class {c}")
    } else {
        s.detail.clone()
    };
    let uri = path_to_uri(&file.path);
    let data = obj(vec![
        ("file", file.path.to_string_lossy().into_owned().into()),
        ("name", s.name.clone().into()),
        ("selStart", s.sel_start.into()),
    ]);
    obj(vec![
        ("name", s.name.clone().into()),
        ("kind", symbol_kind(s, in_class).into()),
        ("detail", detail.into()),
        ("uri", uri.into()),
        ("range", range_json(ws, r.0, s.start, s.end.max(s.sel_end))),
        ("selectionRange", range_json(ws, r.0, s.sel_start, s.sel_end)),
        ("data", data),
    ])
}

fn resolve_item(ws: &Workspace, item: &Json) -> Option<SymRef> {
    let data = item.get("data");
    if let Some(p) = data.get("file").as_str() {
        if let Some(f) = ws.file_id(&PathBuf::from(p)) {
            let sel = data.get("selStart").as_u32().unwrap_or(u32::MAX);
            let name = data.get("name").as_str().unwrap_or("");
            let file = ws.file(f);
            if let Some((i, _)) = file.symbols.iter().enumerate().find(|(_, s)| s.sel_start == sel && s.name == name) {
                return Some((f, i as u32));
            }
            if let Some((i, _)) = file.symbols.iter().enumerate().find(|(_, s)| s.name == name) {
                return Some((f, i as u32));
            }
        }
    }
    let uri = item.get("uri").as_str()?;
    let name = item.get("name").as_str()?;
    let f = ws.file_id(&uri_to_path(uri))?;
    let file = ws.file(f);
    file.symbols.iter().enumerate().find(|(_, s)| s.name == name).map(|(i, _)| (f, i as u32))
}

fn at_cursor(ws: &mut Workspace, p: &Json) -> Option<(usize, u32)> {
    let path = uri_to_path(p.at(&["textDocument", "uri"]).as_str()?);
    let f = match ws.file_id(&path) {
        Some(f) => f,
        None => {
            let t = std::fs::read_to_string(&path).ok()?;
            ws.update(&path, t)
        }
    };
    let line = p.at(&["position", "line"]).as_u32()?;
    let col = p.at(&["position", "character"]).as_u32()?;
    Some((f, ws.file(f).offset(line, col)))
}

// ---- Type Hierarchy ----

pub fn prepare_type_hierarchy(ws: &mut Workspace, p: &Json) -> Json {
    let Some((f, off)) = at_cursor(ws, p) else {
        return Json::Null;
    };
    let mut targets: Vec<SymRef> = Vec::new();
    for r in ws.definitions(f, off) {
        let s = ws.sym(r);
        match s.kind {
            SymKind::Class | SymKind::Interface | SymKind::Module => targets.push(r),
            SymKind::Typedef | SymKind::Variable | SymKind::Port | SymKind::Instance => {
                if let Some(tn) = s.type_name.clone() {
                    let b = bare_type(&tn);
                    targets.extend(ws.scopes_named(b, &[SymKind::Class, SymKind::Interface, SymKind::Module]));
                }
            }
            _ => {}
        }
    }
    if targets.is_empty() {
        // Cursor inside a class/module header whitespace: fall back to innermost enclosing type.
        for &i in ws.file(f).enclosing(off).iter().rev() {
            let s = &ws.file(f).symbols[i as usize];
            if matches!(s.kind, SymKind::Class | SymKind::Interface | SymKind::Module) {
                targets.push((f, i));
                break;
            }
        }
    }
    if targets.is_empty() {
        return Json::Null;
    }
    Json::Arr(targets.into_iter().take(8).map(|r| hierarchy_item(ws, r)).collect())
}

pub fn supertypes(ws: &Workspace, p: &Json) -> Json {
    let item = p.get("item");
    let Some(r) = resolve_item(ws, item) else {
        return Json::Arr(Vec::new());
    };
    let s = ws.sym(r);
    let Some(base) = s.type_name.as_deref().map(bare_type).filter(|b| !b.is_empty()) else {
        return Json::Arr(Vec::new());
    };
    let supers = ws.scopes_named(base, &[SymKind::Class, SymKind::Interface]);
    Json::Arr(supers.into_iter().map(|sr| hierarchy_item(ws, sr)).collect())
}

pub fn subtypes(ws: &Workspace, p: &Json) -> Json {
    let item = p.get("item");
    let Some(r) = resolve_item(ws, item) else {
        return Json::Arr(Vec::new());
    };
    let target_name = ws.sym(r).name.clone();
    let mut subs: Vec<SymRef> = Vec::new();
    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        for (si, sym) in fx.symbols.iter().enumerate() {
            if matches!(sym.kind, SymKind::Class | SymKind::Interface) {
                if let Some(base) = sym.type_name.as_deref() {
                    if bare_type(base) == target_name {
                        subs.push((fi, si as u32));
                    }
                }
            }
        }
    }
    subs.sort_by(|&a, &b| ws.sym(a).name.cmp(&ws.sym(b).name));
    Json::Arr(subs.into_iter().map(|sr| hierarchy_item(ws, sr)).collect())
}

// ---- Call Hierarchy ----

pub fn prepare_call_hierarchy(ws: &mut Workspace, p: &Json) -> Json {
    let Some((f, off)) = at_cursor(ws, p) else {
        return Json::Null;
    };
    let mut targets: Vec<SymRef> = Vec::new();
    for r in ws.definitions(f, off) {
        if matches!(ws.sym(r).kind, SymKind::Function | SymKind::Task) {
            targets.push(r);
        }
    }
    if targets.is_empty() {
        for &i in ws.file(f).enclosing(off).iter().rev() {
            if matches!(ws.file(f).symbols[i as usize].kind, SymKind::Function | SymKind::Task) {
                targets.push((f, i));
                break;
            }
        }
    }
    if targets.is_empty() {
        return Json::Null;
    }
    Json::Arr(targets.into_iter().take(8).map(|r| hierarchy_item(ws, r)).collect())
}

fn is_call_at(text: &str, end_byte: usize) -> bool {
    let bytes = text.as_bytes();
    let mut i = end_byte;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    // `foo(...)` or task call `foo;`
    matches!(bytes.get(i), Some(b'(' | b';'))
}

/// Returns `[start..end]` body spans for routine `r` (including out-of-block
/// `Class::method` implementations when `r` is an `extern` prototype).
fn routine_spans(ws: &Workspace, r: SymRef) -> Vec<(usize, u32, u32)> {
    let s = ws.sym(r);
    let mut spans = vec![(r.0, s.start, s.end)];
    if let Some(parent_idx) = s.parent {
        let parent = &ws.file(r.0).symbols[parent_idx as usize];
        if parent.kind == SymKind::Class {
            for b in ws.bodies_of(&parent.name, &s.name) {
                if b != r {
                    let bs = ws.sym(b);
                    spans.push((b.0, bs.start, bs.end));
                }
            }
        }
    }
    spans
}

pub fn outgoing_calls(ws: &Workspace, p: &Json) -> Json {
    let item = p.get("item");
    let Some(r) = resolve_item(ws, item) else {
        return Json::Arr(Vec::new());
    };
    let mut grouped: Vec<(SymRef, Vec<Json>)> = Vec::new();
    let mut index_by_target: std::collections::HashMap<SymRef, usize> = std::collections::HashMap::new();

    for (fi, start, end) in routine_spans(ws, r) {
        let file = ws.file(fi);
        let self_sel = ws.sym(r).sel_start;
        let bytes = file.text.as_bytes();
        let mut pos = start as usize;
        let limit = (end as usize).min(bytes.len());
        while pos < limit {
            if is_ident_byte(bytes[pos]) && !bytes[pos].is_ascii_digit() {
                let id_start = pos;
                while pos < limit && is_ident_byte(bytes[pos]) {
                    pos += 1;
                }
                let id_end = pos;
                if id_start as u32 == self_sel && fi == r.0 {
                    continue;
                }
                if !is_call_at(&file.text, id_end) {
                    continue;
                }
                let word = &file.text[id_start..id_end];
                if word.starts_with('$') {
                    continue;
                }
                let defs = ws.definitions(fi, id_start as u32);
                let callee = defs
                    .into_iter()
                    .find(|&d| matches!(ws.sym(d).kind, SymKind::Function | SymKind::Task) && d != r)
                    .or_else(|| {
                        ws.globals
                            .get(word)?
                            .iter()
                            .copied()
                            .find(|&d| matches!(ws.sym(d).kind, SymKind::Function | SymKind::Task) && d != r)
                    });
                if let Some(target) = callee {
                    let rng = range_json(ws, fi, id_start as u32, id_end as u32);
                    if let Some(&idx) = index_by_target.get(&target) {
                        grouped[idx].1.push(rng);
                    } else {
                        index_by_target.insert(target, grouped.len());
                        grouped.push((target, vec![rng]));
                    }
                }
            } else {
                // Skip comments & strings cleanly so commented names aren't treated as calls.
                if bytes[pos] == b'/' && bytes.get(pos + 1) == Some(&b'/') {
                    while pos < limit && bytes[pos] != b'\n' {
                        pos += 1;
                    }
                } else if bytes[pos] == b'/' && bytes.get(pos + 1) == Some(&b'*') {
                    pos += 2;
                    while pos + 1 < limit && !(bytes[pos] == b'*' && bytes[pos + 1] == b'/') {
                        pos += 1;
                    }
                    pos += 2;
                } else if bytes[pos] == b'"' {
                    pos += 1;
                    while pos < limit && bytes[pos] != b'"' {
                        if bytes[pos] == b'\\' {
                            pos += 1;
                        }
                        pos += 1;
                    }
                    pos += 1;
                } else {
                    pos += 1;
                }
            }
        }
    }

    Json::Arr(
        grouped
            .into_iter()
            .map(|(target, ranges)| obj(vec![("to", hierarchy_item(ws, target)), ("fromRanges", Json::Arr(ranges))]))
            .collect(),
    )
}

pub fn incoming_calls(ws: &Workspace, p: &Json) -> Json {
    let item = p.get("item");
    let Some(target) = resolve_item(ws, item) else {
        return Json::Arr(Vec::new());
    };
    let target_name = ws.sym(target).name.clone();
    let len = target_name.len() as u32;
    let mut grouped: Vec<(SymRef, Vec<Json>)> = Vec::new();
    let mut index_by_caller: std::collections::HashMap<SymRef, usize> = std::collections::HashMap::new();
    let mut target_decl_locs: HashSet<(usize, u32)> = HashSet::new();
    target_decl_locs.insert((target.0, ws.sym(target).sel_start));

    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        let Some(offs) = fx.idents.get(&target_name) else { continue };
        for &off in offs {
            if target_decl_locs.contains(&(fi, off)) {
                continue;
            }
            if !is_call_at(&fx.text, (off + len) as usize) {
                continue;
            }
            // Find the innermost enclosing Function/Task (else enclosing Module/Class/Program).
            let enc = fx.enclosing(off);
            let caller_idx = enc
                .iter()
                .rev()
                .copied()
                .find(|&i| matches!(fx.symbols[i as usize].kind, SymKind::Function | SymKind::Task))
                .or_else(|| enc.last().copied());
            let Some(ci) = caller_idx else { continue };
            let caller = (fi, ci);
            if caller == target {
                continue;
            }
            let cs = ws.sym(caller);
            if off == cs.sel_start {
                continue;
            }
            let rng = range_json(ws, fi, off, off + len);
            if let Some(&idx) = index_by_caller.get(&caller) {
                grouped[idx].1.push(rng);
            } else {
                index_by_caller.insert(caller, grouped.len());
                grouped.push((caller, vec![rng]));
            }
        }
    }

    Json::Arr(
        grouped
            .into_iter()
            .map(|(caller, ranges)| obj(vec![("from", hierarchy_item(ws, caller)), ("fromRanges", Json::Arr(ranges))]))
            .collect(),
    )
}

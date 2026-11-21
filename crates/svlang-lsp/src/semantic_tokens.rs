//! Semantic token classification (`textDocument/semanticTokens/full`) and
//! inactive preprocessor conditional regions (`svlang/inactiveRanges`).

use std::collections::HashMap;

use svlang::syntax::{CondKind, Options, TokenKind};

use crate::index::{SymKind, Symbol};
use crate::json::{obj, Json};
use crate::text::uri_to_path;
use crate::workspace::Workspace;

// Token type indices in `TOKEN_TYPES` (must stay in sync with `legend()`).
const TT_NAMESPACE: u32 = 0; // module, program, package, checker
const TT_CLASS: u32 = 1; // class
const TT_INTERFACE: u32 = 2; // interface, modport
const TT_TYPE: u32 = 3; // typedef
const TT_ENUM_MEMBER: u32 = 4; // enum value
const TT_PARAMETER: u32 = 5; // parameter, localparam, genvar
const TT_VARIABLE: u32 = 6; // variable, signal, local
const TT_PROPERTY: u32 = 7; // class property, port, instance
const TT_FUNCTION: u32 = 8; // standalone/package/module function or task, system task
const TT_METHOD: u32 = 9; // class method, constraint, covergroup, property, sequence
const TT_MACRO: u32 = 10; // `define / `macro use

pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "class",
    "interface",
    "type",
    "enumMember",
    "parameter",
    "variable",
    "property",
    "function",
    "method",
    "macro",
];

// Token modifier bit positions in `TOKEN_MODIFIERS`.
const MOD_DECLARATION: u32 = 1 << 0;
const MOD_READONLY: u32 = 1 << 1;
const MOD_STATIC: u32 = 1 << 2;
const MOD_MODIFICATION: u32 = 1 << 3;

pub const TOKEN_MODIFIERS: &[&str] = &["declaration", "readonly", "static", "modification"];

/// LSP `SemanticTokensOptions` capability advertisement.
pub fn capabilities() -> Json {
    let types = Json::Arr(TOKEN_TYPES.iter().map(|&s| s.into()).collect());
    let mods = Json::Arr(TOKEN_MODIFIERS.iter().map(|&s| s.into()).collect());
    obj(vec![("legend", obj(vec![("tokenTypes", types), ("tokenModifiers", mods)])), ("full", true.into())])
}

fn classify_symbol(s: &Symbol, in_class: bool, is_decl: bool) -> (u32, u32) {
    let mut mods = if is_decl { MOD_DECLARATION } else { 0 };
    if s.detail.split_whitespace().any(|w| w == "static") {
        mods |= MOD_STATIC;
    }
    if s.detail.split_whitespace().any(|w| matches!(w, "const" | "parameter" | "localparam")) {
        mods |= MOD_READONLY;
    }
    if s.detail.split_whitespace().any(|w| matches!(w, "rand" | "randc")) {
        mods |= MOD_MODIFICATION;
    }
    let ty = match s.kind {
        SymKind::Module | SymKind::Program | SymKind::Package | SymKind::Checker => TT_NAMESPACE,
        SymKind::Class => TT_CLASS,
        SymKind::Interface | SymKind::Modport => TT_INTERFACE,
        SymKind::Typedef => TT_TYPE,
        SymKind::EnumMember => {
            mods |= MOD_READONLY;
            TT_ENUM_MEMBER
        }
        SymKind::Parameter | SymKind::Genvar => {
            mods |= MOD_READONLY;
            TT_PARAMETER
        }
        SymKind::Function | SymKind::Task if in_class => TT_METHOD,
        SymKind::Function | SymKind::Task => TT_FUNCTION,
        SymKind::Constraint | SymKind::Covergroup | SymKind::Property | SymKind::Sequence | SymKind::Clocking => {
            TT_METHOD
        }
        SymKind::Macro => TT_MACRO,
        SymKind::Variable if in_class => TT_PROPERTY,
        SymKind::Port | SymKind::Instance => TT_PROPERTY,
        SymKind::Variable => TT_VARIABLE,
    };
    (ty, mods)
}

#[derive(Clone, Copy)]
struct RawTok {
    line: u32,
    col: u32,
    len: u32,
    ty: u32,
    mods: u32,
}

fn ensure_file(ws: &mut Workspace, uri: &str) -> Option<usize> {
    let path = uri_to_path(uri);
    match ws.file_id(&path) {
        Some(f) => Some(f),
        None => {
            let t = std::fs::read_to_string(&path).ok()?;
            Some(ws.update(&path, t))
        }
    }
}

/// Computes `textDocument/semanticTokens/full` for `uri`.
pub fn semantic_tokens_full(ws: &mut Workspace, uri: &str) -> Json {
    let Some(f) = ensure_file(ws, uri) else {
        return obj(vec![("data", Json::Arr(Vec::new()))]);
    };
    let (path_str, text) = {
        let file = ws.file(f);
        (file.path.to_string_lossy().into_owned(), file.text.clone())
    };
    let mut raw: Vec<RawTok> = Vec::new();
    let mut decl_offsets: HashMap<u32, (u32, u32)> = HashMap::new();

    {
        let file = ws.file(f);
        for s in &file.symbols {
            if s.sel_end <= s.sel_start {
                continue;
            }
            let in_class =
                s.parent.is_some_and(|p| file.symbols[p as usize].kind == SymKind::Class) || s.scope_class.is_some();
            let (ty, mods) = classify_symbol(s, in_class, true);
            let (l0, c0) = file.position(s.sel_start);
            let (l1, c1) = file.position(s.sel_end);
            if l0 == l1 && c1 > c0 {
                decl_offsets.insert(s.sel_start, (ty, mods));
                raw.push(RawTok { line: l0, col: c0, len: c1 - c0, ty, mods });
            }
        }
    }

    // Parse single file without expanding external includes so byte offsets stay in this file.
    let tree = svlang::syntax::parse_source(&text, &path_str, Options { max_include_depth: 1, ..Default::default() });
    let mut seen_offsets: HashMap<u32, ()> = decl_offsets.keys().map(|&k| (k, ())).collect();

    // Macro invocations (`uvm_info, `MY_MACRO, etc.) in this file.
    {
        let file = ws.file(f);
        for m in &tree.macro_uses {
            if m.origin.file != 0 {
                continue;
            }
            let start = m.origin.offset;
            let end = (start as usize + 1 + m.name.len()).min(text.len()) as u32;
            if seen_offsets.insert(start, ()).is_some() {
                continue;
            }
            let (l0, c0) = file.position(start);
            let (l1, c1) = file.position(end);
            if l0 == l1 && c1 > c0 {
                raw.push(RawTok { line: l0, col: c0, len: c1 - c0, ty: TT_MACRO, mods: 0 });
            }
        }
    }

    // Classify identifier & system-task tokens via Workspace resolution.
    for tok in &tree.tokens {
        if tok.from_macro || tok.origin.file != 0 {
            continue;
        }
        let start = tok.origin.offset;
        if seen_offsets.contains_key(&start) {
            continue;
        }
        match tok.kind {
            TokenKind::SystemIdentifier => {
                let file = ws.file(f);
                let end = start + tok.text.len() as u32;
                let (l0, c0) = file.position(start);
                let (l1, c1) = file.position(end);
                if l0 == l1 && c1 > c0 {
                    seen_offsets.insert(start, ());
                    raw.push(RawTok { line: l0, col: c0, len: c1 - c0, ty: TT_FUNCTION, mods: 0 });
                }
            }
            TokenKind::Identifier | TokenKind::EscapedIdentifier => {
                let defs = ws.definitions(f, start);
                let (ty, mods) = if let Some(&r) = defs.first() {
                    let s = ws.sym(r);
                    let target_file = ws.file(r.0);
                    let in_class = s.parent.is_some_and(|p| target_file.symbols[p as usize].kind == SymKind::Class)
                        || s.scope_class.is_some();
                    classify_symbol(s, in_class, false)
                } else if tok.text.starts_with("uvm_")
                    && (tok.text.ends_with("_t") || tok.text.ends_with("_e") || tok.text.ends_with("_enum"))
                {
                    (TT_TYPE, 0)
                } else if tok.text.as_ref() == "uvm_pkg" {
                    (TT_NAMESPACE, 0)
                } else if tok.text.starts_with("uvm_") {
                    (TT_CLASS, 0)
                } else if tok.text.starts_with("UVM_") {
                    (TT_ENUM_MEMBER, MOD_READONLY)
                } else {
                    continue;
                };
                let file = ws.file(f);
                let end = start + tok.text.len() as u32;
                let (l0, c0) = file.position(start);
                let (l1, c1) = file.position(end);
                if l0 == l1 && c1 > c0 {
                    seen_offsets.insert(start, ());
                    raw.push(RawTok { line: l0, col: c0, len: c1 - c0, ty, mods });
                }
            }
            _ => {}
        }
    }

    raw.sort_by_key(|t| (t.line, t.col));
    let mut data: Vec<Json> = Vec::with_capacity(raw.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;
    let mut last_end: Option<(u32, u32)> = None;
    for t in raw {
        if let Some((ll, lc)) = last_end {
            if t.line == ll && t.col < lc {
                continue; // never emit overlapping tokens
            }
        }
        let d_line = t.line - prev_line;
        let d_start = if d_line == 0 { t.col - prev_col } else { t.col };
        data.push(d_line.into());
        data.push(d_start.into());
        data.push(t.len.into());
        data.push(t.ty.into());
        data.push(t.mods.into());
        prev_line = t.line;
        prev_col = t.col;
        last_end = Some((t.line, t.col + t.len));
    }

    obj(vec![("data", Json::Arr(data))])
}

/// Computes untaken preprocessor conditional regions (`#ifdef` / `` `ifdef ``,
/// `` `ifndef ``, `` `elsif ``, `` `else ``) in `uri` for editor dimming,
/// honoring active preprocessor `defines`.
pub fn inactive_ranges(ws: &mut Workspace, uri: &str, defines: &[String]) -> Json {
    let Some(f) = ensure_file(ws, uri) else {
        return Json::Arr(Vec::new());
    };
    let (path_str, text, lines) = {
        let file = ws.file(f);
        (file.path.to_string_lossy().into_owned(), file.text.clone(), file.lines.clone())
    };
    let tree = svlang::syntax::parse_source(
        &text,
        &path_str,
        Options { max_include_depth: 1, defines: defines.to_vec(), ..Default::default() },
    );

    // Stack of `(branch_taken, inactive_start_byte_offset)` per conditional nesting level.
    let mut stack: Vec<(bool, Option<u32>)> = Vec::new();
    let mut spans: Vec<(u32, u32)> = Vec::new();

    let next_line_start = |off: u32| -> u32 {
        let s = (off as usize).min(text.len());
        match text[s..].find('\n') {
            Some(nl) => (s + nl + 1) as u32,
            None => text.len() as u32,
        }
    };

    for ev in tree.conditionals.iter().filter(|e| e.origin.file == 0) {
        let outer_active = stack.iter().all(|(taken, _)| *taken);
        match ev.kind {
            CondKind::Ifdef | CondKind::Ifndef => {
                let effective = outer_active && ev.taken;
                let inactive_from =
                    if outer_active && !ev.taken { Some(next_line_start(ev.origin.offset)) } else { None };
                stack.push((effective, inactive_from));
            }
            CondKind::Elsif | CondKind::Else => {
                if let Some((_prev_taken, prev_inactive_start)) = stack.pop() {
                    let parent_active = stack.iter().all(|(taken, _)| *taken);
                    if let Some(start) = prev_inactive_start {
                        if ev.origin.offset > start {
                            spans.push((start, ev.origin.offset));
                        }
                    }
                    let effective = parent_active && ev.taken;
                    let inactive_from =
                        if parent_active && !ev.taken { Some(next_line_start(ev.origin.offset)) } else { None };
                    stack.push((effective, inactive_from));
                }
            }
            CondKind::Endif => {
                if let Some((_, Some(start))) = stack.pop() {
                    if ev.origin.offset > start {
                        spans.push((start, ev.origin.offset));
                    }
                }
            }
        }
    }

    let ranges: Vec<Json> = spans
        .into_iter()
        .filter_map(|(a, b)| {
            let (l0, c0) = lines.position(&text, a);
            let (l1, c1) = lines.position(&text, b);
            if (l1, c1) <= (l0, c0) {
                return None;
            }
            Some(obj(vec![
                ("start", obj(vec![("line", l0.into()), ("character", c0.into())])),
                ("end", obj(vec![("line", l1.into()), ("character", c1.into())])),
            ]))
        })
        .collect();
    Json::Arr(ranges)
}

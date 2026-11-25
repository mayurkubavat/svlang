//! RTL Module/Interface Hierarchy (`svlang/rtlHierarchy`) and Signal Driver/Load
//! Tracing (`svlang/traceSignal`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use svlang::syntax::{NodeId, Options, SyntaxKind as K, Tree};

use crate::index::SymKind;
use crate::json::{obj, Json};
use crate::text::{path_to_uri, uri_to_path, word_at};
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

fn bare_module_name(raw: &str) -> &str {
    let no_params = raw.split('#').next().unwrap_or(raw).trim();
    no_params.rsplit("::").next().unwrap_or(no_params).trim()
}

fn is_library_path(path: &Path, libraries: &[PathBuf]) -> bool {
    libraries.iter().any(|lib| path.starts_with(lib))
}

// ---- RTL Module / Interface Instantiation Hierarchy ----

const MAX_ROOT_MODULES: usize = 250;

fn module_has_instances(ws: &Workspace, mod_ref: SymRef) -> bool {
    ws.file(mod_ref.0)
        .symbols
        .iter()
        .any(|s| s.kind == SymKind::Instance && s.parent == Some(mod_ref.1) && s.type_name.is_some())
}

/// Lazy 1-level-at-a-time RTL hierarchy:
/// - Without `moduleName`: returns root modules (with `hasChildren: bool`, no recursive children),
///   scoped to `activeUri`'s enclosing git/project root when inside a multi-repo workspace.
/// - With `moduleName`: returns direct child instances of `moduleName` (`{"children": [...]}`).
pub fn rtl_hierarchy(ws: &Workspace, libraries: &[PathBuf], p: &Json) -> Json {
    let active_path = p.get("activeUri").as_str().filter(|s| !s.is_empty()).map(uri_to_path);
    let ancestors: HashSet<String> =
        p.get("ancestors").as_arr().iter().filter_map(|v| v.as_str().map(String::from)).collect();

    if let Some(target_mod) = p.get("moduleName").as_str().filter(|s| !s.is_empty()) {
        let anchor_f = active_path.as_deref().and_then(|ap| ws.file_id(ap));
        let mod_ref = anchor_f
            .and_then(|f| {
                ws.file(f)
                    .symbols
                    .iter()
                    .enumerate()
                    .find(|(_, s)| {
                        matches!(s.kind, SymKind::Module | SymKind::Interface | SymKind::Program)
                            && s.name == target_mod
                    })
                    .map(|(si, _)| (f, si as u32))
                    .or_else(|| {
                        ws.scopes_named_near(f, target_mod, &[SymKind::Module, SymKind::Interface, SymKind::Program])
                            .into_iter()
                            .next()
                    })
            })
            .or_else(|| {
                ws.scopes_named(target_mod, &[SymKind::Module, SymKind::Interface, SymKind::Program])
                    .into_iter()
                    .find(|&r| !is_library_path(&ws.file(r.0).path, libraries))
            });

        let Some(mod_ref) = mod_ref else {
            return obj(vec![("children", Json::Arr(Vec::new()))]);
        };

        let mod_file = ws.file(mod_ref.0);
        let inst_uri = path_to_uri(&mod_file.path);
        let mut kids_json = Vec::new();
        for (si, sym) in mod_file.symbols.iter().enumerate() {
            if sym.kind != SymKind::Instance || sym.parent != Some(mod_ref.1) {
                continue;
            }
            let child_mod = sym.type_name.as_deref().map(bare_module_name).unwrap_or("");
            if child_mod.is_empty() {
                continue;
            }
            let site_range = range_json(ws, mod_ref.0, sym.sel_start, sym.sel_end);
            let target_def = ws
                .scopes_named_near(mod_ref.0, child_mod, &[SymKind::Module, SymKind::Interface, SymKind::Program])
                .into_iter()
                .find(|&r| !is_library_path(&ws.file(r.0).path, libraries));
            let def_uri = target_def.map(|r| path_to_uri(&ws.file(r.0).path)).unwrap_or_else(|| inst_uri.clone());
            let def_range = target_def
                .map(|r| {
                    let ds = ws.sym(r);
                    range_json(ws, r.0, ds.sel_start, ds.sel_end)
                })
                .unwrap_or_else(|| site_range.clone());
            let kind_label = target_def.map(|r| ws.sym(r).kind.label()).unwrap_or("module");
            let has_children = !ancestors.contains(child_mod)
                && child_mod != target_mod
                && target_def.is_some_and(|td| module_has_instances(ws, td));
            let _ = si;
            kids_json.push(obj(vec![
                ("instanceName", sym.name.clone().into()),
                ("moduleName", child_mod.into()),
                ("kind", kind_label.into()),
                ("uri", inst_uri.clone().into()),
                ("range", site_range),
                ("definitionUri", def_uri.into()),
                ("definitionRange", def_range),
                ("hasChildren", has_children.into()),
            ]));
        }
        return obj(vec![("children", Json::Arr(kids_json))]);
    }

    // Root query: scope to `activeUri`'s enclosing git/project root when that root holds RTL modules.
    let active_proj = active_path.as_deref().and_then(|ap| ws.project_root_for_path(ap));
    let use_proj_scope = active_proj.as_ref().is_some_and(|proj| {
        ws.files.iter().flatten().any(|fx| {
            !is_library_path(&fx.path, libraries)
                && fx.path.starts_with(proj)
                && fx.symbols.iter().any(|s| matches!(s.kind, SymKind::Module | SymKind::Interface | SymKind::Program))
        })
    });

    let in_scope = |path: &Path| -> bool {
        if is_library_path(path, libraries) {
            return false;
        }
        if use_proj_scope {
            if let Some(proj) = &active_proj {
                return path.starts_with(proj);
            }
        }
        true
    };

    let mut modules: HashMap<String, SymRef> = HashMap::new();
    let mut instantiated: HashSet<String> = HashSet::new();

    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        if !in_scope(&fx.path) {
            continue;
        }
        for (si, sym) in fx.symbols.iter().enumerate() {
            if matches!(sym.kind, SymKind::Module | SymKind::Interface | SymKind::Program) {
                modules.entry(sym.name.clone()).or_insert((fi, si as u32));
            } else if sym.kind == SymKind::Instance {
                let target_mod = sym.type_name.as_deref().map(bare_module_name).unwrap_or("");
                if !target_mod.is_empty() {
                    instantiated.insert(target_mod.to_string());
                }
            }
        }
    }

    let mut roots: Vec<SymRef> = modules
        .iter()
        .filter(|(name, &r)| ws.sym(r).kind == SymKind::Module && !instantiated.contains(*name))
        .map(|(_, &r)| r)
        .collect();
    if roots.is_empty() {
        roots = modules.values().copied().collect();
    }
    roots.sort_by(|&a, &b| ws.sym(a).name.cmp(&ws.sym(b).name));
    let total_roots = roots.len();
    roots.truncate(MAX_ROOT_MODULES);

    let root_nodes: Vec<Json> = roots
        .into_iter()
        .map(|r| {
            let sym = ws.sym(r);
            let uri = path_to_uri(&ws.file(r.0).path);
            let range = range_json(ws, r.0, sym.sel_start, sym.sel_end);
            let has_children = module_has_instances(ws, r);
            obj(vec![
                ("instanceName", sym.name.clone().into()),
                ("moduleName", sym.name.clone().into()),
                ("kind", sym.kind.label().into()),
                ("uri", uri.clone().into()),
                ("range", range.clone()),
                ("definitionUri", uri.into()),
                ("definitionRange", range),
                ("hasChildren", has_children.into()),
            ])
        })
        .collect();

    obj(vec![
        ("roots", Json::Arr(root_nodes)),
        ("totalRoots", (total_roots as u32).into()),
        ("moduleCount", (modules.len() as u32).into()),
    ])
}

// ---- Signal / Variable Driver & Load Tracing ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Declaration,
    Driver,
    Load,
}

pub fn trace_signal(ws: &Workspace, p: &Json) -> Json {
    let Some(uri) = p.at(&["textDocument", "uri"]).as_str() else {
        return Json::Null;
    };
    let path = uri_to_path(uri);
    let Some(f) = ws.file_id(&path) else {
        return Json::Null;
    };
    let line = p.at(&["position", "line"]).as_u32().unwrap_or(0);
    let col = p.at(&["position", "character"]).as_u32().unwrap_or(0);
    let off = ws.file(f).offset(line, col);
    let Some((ws_s, ws_e)) = word_at(&ws.file(f).text, off) else {
        return Json::Null;
    };
    let target_name = ws.file(f).text[ws_s..ws_e].to_string();
    if target_name.is_empty() || target_name.starts_with(['`', '$']) {
        return Json::Null;
    }

    let defs = ws.definitions(f, off);
    let primary_def = defs.first().copied();
    let sym_info = if let Some(d) = primary_def {
        let s = ws.sym(d);
        obj(vec![
            ("name", s.name.clone().into()),
            ("kind", s.kind.label().into()),
            ("detail", s.detail.clone().into()),
            ("uri", path_to_uri(&ws.file(d.0).path).into()),
            ("range", range_json(ws, d.0, s.sel_start, s.sel_end)),
        ])
    } else {
        obj(vec![
            ("name", target_name.clone().into()),
            ("kind", "signal".into()),
            ("detail", target_name.clone().into()),
            ("uri", uri.into()),
            ("range", range_json(ws, f, ws_s as u32, ws_e as u32)),
        ])
    };

    // Scope restriction: if the symbol is local to a single module/function/class
    // (not a class member or package item), prefer occurrences inside that scope first,
    // while also capturing hierarchical / member accesses across workspace files.
    let restrict_scope: Option<(usize, u32, u32)> = primary_def.and_then(|d| {
        let s = ws.sym(d);
        if let Some(p_idx) = s.parent {
            let parent = &ws.file(d.0).symbols[p_idx as usize];
            if matches!(parent.kind, SymKind::Module | SymKind::Function | SymKind::Task) {
                return Some((d.0, parent.start, parent.end));
            }
        }
        None
    });

    // Map `(module_name, port_name) -> is_output_or_inout` so `.out_port(sig)` is classified as a Driver!
    let mut port_drives_arg: HashMap<(String, String), bool> = HashMap::new();
    for fx in ws.files.iter().flatten() {
        for sym in &fx.symbols {
            if sym.kind == SymKind::Port {
                if let Some(p_idx) = sym.parent {
                    let parent = &fx.symbols[p_idx as usize];
                    let is_out = sym.detail.split_whitespace().any(|w| matches!(w, "output" | "inout"));
                    port_drives_arg.insert((parent.name.clone(), sym.name.clone()), is_out);
                }
            }
        }
    }

    let mut declarations = Vec::new();
    let mut drivers = Vec::new();
    let mut loads = Vec::new();

    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        let Some(offs) = fx.idents.get(&target_name) else { continue };
        if let Some((only_f, _, _)) = restrict_scope {
            if fi != only_f {
                continue;
            }
        }

        let tree = svlang::syntax::parse_source(
            &fx.text,
            &fx.path.to_string_lossy(),
            Options { max_include_depth: 1, ..Default::default() },
        );
        let roles = classify_file_uses(&tree, &target_name, fx, &port_drives_arg);

        for &o in offs {
            if let Some((_, s0, s1)) = restrict_scope {
                if o < s0 || o > s1 {
                    continue;
                }
            }
            let len = target_name.len() as u32;
            let (ln, _) = fx.position(o);
            let line_text = fx.text.lines().nth(ln as usize).unwrap_or("").trim().to_string();
            let container = fx
                .enclosing(o)
                .last()
                .map(|&idx| {
                    let cs = &fx.symbols[idx as usize];
                    format!("{} {}", cs.kind.label(), cs.name)
                })
                .unwrap_or_default();

            let is_decl_sym = fx.symbols.iter().find(|s| s.sel_start == o && s.name == target_name);
            let role = if let Some(ds) = is_decl_sym {
                // `output logic y = ...` or `logic x = expr`: record declaration, and if it has `= expr` it's also an initial/continuous driver!
                if ds.detail.contains('=') && !matches!(ds.kind, SymKind::Parameter | SymKind::Typedef) {
                    drivers.push(hit_json(ws, fi, o, len, ln + 1, &line_text, &container, "initializer"));
                }
                Role::Declaration
            } else {
                roles.get(&o).copied().unwrap_or(Role::Load)
            };

            let detail_kind = match role {
                Role::Declaration => "declaration",
                Role::Driver => "driver",
                Role::Load => "load",
            };
            let entry = hit_json(ws, fi, o, len, ln + 1, &line_text, &container, detail_kind);
            match role {
                Role::Declaration => declarations.push(entry),
                Role::Driver => drivers.push(entry),
                Role::Load => loads.push(entry),
            }
        }
    }

    obj(vec![
        ("symbol", sym_info),
        ("declarations", Json::Arr(declarations)),
        ("drivers", Json::Arr(drivers)),
        ("loads", Json::Arr(loads)),
    ])
}

#[allow(clippy::too_many_arguments)]
fn hit_json(
    ws: &Workspace,
    fi: usize,
    off: u32,
    len: u32,
    line_1based: u32,
    line_text: &str,
    container: &str,
    kind: &str,
) -> Json {
    let file = ws.file(fi);
    let fname = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    obj(vec![
        ("uri", path_to_uri(&file.path).into()),
        ("fileName", fname.into()),
        ("line", line_1based.into()),
        ("range", range_json(ws, fi, off, off + len)),
        ("lineText", line_text.into()),
        ("container", container.into()),
        ("kind", kind.into()),
    ])
}

/// Walks the CST of `file` and records every byte offset where `target_name`
/// appears in a Driver (write) position; everything else defaults to Load.
fn classify_file_uses(
    tree: &Tree,
    target_name: &str,
    fx: &crate::index::FileIndex,
    port_drives_arg: &HashMap<(String, String), bool>,
) -> HashMap<u32, Role> {
    let mut map = HashMap::new();

    fn mark_lhs_ident(tree: &Tree, n: NodeId, target_name: &str, map: &mut HashMap<u32, Role>) {
        match tree.kind(n) {
            K::NameExpr | K::Name => {
                for t in tree.token_range(n) {
                    let tok = &tree.tokens[t as usize];
                    if !tok.from_macro && tok.origin.file == 0 && tok.text.as_ref() == target_name {
                        map.insert(tok.origin.offset, Role::Driver);
                    }
                }
            }
            K::SelectExpr => {
                if let Some(base) = tree.children(n).next() {
                    mark_lhs_ident(tree, base, target_name, map);
                }
            }
            K::MemberExpr | K::ScopedExpr => {
                // In `obj.field = x`, `field` is written (and `obj`'s member is mutated);
                // mark the terminal member if it matches `target_name`, or base if `this.target`.
                let toks = tree.token_range(n);
                if let Some(&last_t) = toks.iter().rev().find(|&&t| {
                    let tok = &tree.tokens[t as usize];
                    !tok.from_macro && tok.origin.file == 0 && tok.kind == svlang::syntax::TokenKind::Identifier
                }) {
                    if tree.tokens[last_t as usize].text.as_ref() == target_name {
                        map.insert(tree.tokens[last_t as usize].origin.offset, Role::Driver);
                    }
                }
            }
            K::ConcatExpr => {
                // `{carry, sum} = a + b;` drives every element of the LHS concatenation!
                for ch in tree.children(n) {
                    mark_lhs_ident(tree, ch, target_name, map);
                }
            }
            _ => {}
        }
    }

    for n in tree.preorder() {
        match tree.kind(n) {
            K::ContinuousAssign | K::ProcAssign => {
                // Continuous `assign a = b;` or procedural `assign`/`force`
                for ch in tree.children(n) {
                    if tree.kind(ch) == K::BinaryExpr {
                        if let Some(lhs) = tree.children(ch).next() {
                            mark_lhs_ident(tree, lhs, target_name, &mut map);
                        }
                    }
                }
            }
            K::BinaryExpr => {
                let mut is_assign = false;
                for child in &tree.node(n).children {
                    if let svlang::syntax::Child::Token(t) = *child {
                        if matches!(
                            tree.tokens[t as usize].text.as_ref(),
                            "=" | "<="
                                | "+="
                                | "-="
                                | "*="
                                | "/="
                                | "%="
                                | "&="
                                | "|="
                                | "^="
                                | "<<="
                                | ">>="
                                | "<<<="
                                | ">>>="
                        ) {
                            is_assign = true;
                            break;
                        }
                    }
                }
                if is_assign {
                    if let Some(lhs) = tree.children(n).next() {
                        mark_lhs_ident(tree, lhs, target_name, &mut map);
                    }
                }
            }
            K::PrefixExpr | K::PostfixExpr => {
                let is_inc_dec = tree.node(n).children.iter().any(|ch| {
                    matches!(ch, svlang::syntax::Child::Token(t) if matches!(tree.tokens[*t as usize].text.as_ref(), "++" | "--"))
                });
                if is_inc_dec {
                    if let Some(opnd) = tree.children(n).next() {
                        mark_lhs_ident(tree, opnd, target_name, &mut map);
                    }
                }
            }
            K::Arg => {
                // `.port_name(expr)` inside an Instance: if `port_name` is an output/inout of the instantiated module, `expr` is driven!
                let toks = tree.token_range(n);
                let mut port_name: Option<String> = None;
                for (idx, &t) in toks.iter().enumerate() {
                    if tree.tokens[t as usize].text.as_ref() == "." {
                        if let Some(&nt) = toks.get(idx + 1) {
                            port_name = Some(tree.tokens[nt as usize].text.to_string());
                        }
                        break;
                    }
                }
                if let Some(pname) = port_name {
                    if let Some(&first_t) = toks.first() {
                        let off = tree.tokens[first_t as usize].origin.offset;
                        if let Some(inst_sym) =
                            fx.symbols.iter().find(|s| s.kind == SymKind::Instance && s.start <= off && off <= s.end)
                        {
                            if let Some(mod_ty) = inst_sym.type_name.as_deref() {
                                let mname = bare_module_name(mod_ty).to_string();
                                if port_drives_arg.get(&(mname, pname)).copied() == Some(true) {
                                    for ch in tree.children(n) {
                                        mark_lhs_ident(tree, ch, target_name, &mut map);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    map
}

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

pub fn rtl_hierarchy(ws: &Workspace, libraries: &[PathBuf]) -> Json {
    let mut modules: HashMap<String, SymRef> = HashMap::new();
    let mut children_of: HashMap<SymRef, Vec<(String, String, SymRef)>> = HashMap::new();
    let mut instantiated: HashSet<String> = HashSet::new();

    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        if is_library_path(&fx.path, libraries) {
            continue;
        }
        for (si, sym) in fx.symbols.iter().enumerate() {
            if matches!(sym.kind, SymKind::Module | SymKind::Interface | SymKind::Program) {
                modules.entry(sym.name.clone()).or_insert((fi, si as u32));
            }
        }
    }

    for (fi, fx) in ws.files.iter().enumerate() {
        let Some(fx) = fx else { continue };
        if is_library_path(&fx.path, libraries) {
            continue;
        }
        for (si, sym) in fx.symbols.iter().enumerate() {
            if sym.kind == SymKind::Instance {
                let Some(parent_idx) = sym.parent else { continue };
                let parent_ref = (fi, parent_idx);
                let target_mod = sym.type_name.as_deref().map(bare_module_name).unwrap_or("").to_string();
                if !target_mod.is_empty() {
                    instantiated.insert(target_mod.clone());
                    children_of.entry(parent_ref).or_default().push((sym.name.clone(), target_mod, (fi, si as u32)));
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

    #[allow(clippy::too_many_arguments)]
    fn build_node(
        ws: &Workspace,
        inst_name: Option<&str>,
        mod_name: &str,
        site: SymRef,
        modules: &HashMap<String, SymRef>,
        children_of: &HashMap<SymRef, Vec<(String, String, SymRef)>>,
        ancestors: &mut HashSet<String>,
        depth: usize,
    ) -> Json {
        let sym = ws.sym(site);
        let file = ws.file(site.0);
        let uri = path_to_uri(&file.path);
        let range = range_json(ws, site.0, sym.sel_start, sym.sel_end);
        let mut kids_json = Vec::new();
        if depth < 16 && ancestors.insert(mod_name.to_string()) {
            if let Some(&mod_ref) = modules.get(mod_name) {
                let mod_file = ws.file(mod_ref.0);
                // Also include Ports summary count on the module node for quick context.
                if let Some(insts) = children_of.get(&mod_ref) {
                    for (child_inst, child_mod, child_site) in insts {
                        kids_json.push(build_node(
                            ws,
                            Some(child_inst),
                            child_mod,
                            *child_site,
                            modules,
                            children_of,
                            ancestors,
                            depth + 1,
                        ));
                    }
                }
                let _ = mod_file;
            }
            ancestors.remove(mod_name);
        }
        let target_def = modules.get(mod_name).copied();
        let def_uri = target_def.map(|r| path_to_uri(&ws.file(r.0).path)).unwrap_or_else(|| uri.clone());
        let def_range = target_def
            .map(|r| {
                let ds = ws.sym(r);
                range_json(ws, r.0, ds.sel_start, ds.sel_end)
            })
            .unwrap_or_else(|| range.clone());
        let kind_label = target_def.map(|r| ws.sym(r).kind.label()).unwrap_or("module");
        obj(vec![
            ("instanceName", inst_name.unwrap_or(mod_name).into()),
            ("moduleName", mod_name.into()),
            ("kind", kind_label.into()),
            ("uri", uri.into()),
            ("range", range),
            ("definitionUri", def_uri.into()),
            ("definitionRange", def_range),
            ("children", Json::Arr(kids_json)),
        ])
    }

    let mut ancestors = HashSet::new();
    let root_nodes: Vec<Json> = roots
        .into_iter()
        .map(|r| {
            let name = ws.sym(r).name.clone();
            build_node(ws, None, &name, r, &modules, &children_of, &mut ancestors, 0)
        })
        .collect();

    obj(vec![("roots", Json::Arr(root_nodes)), ("moduleCount", (modules.len() as u32).into())])
}

// ---- Signal / Variable Driver & Load Tracing ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Declaration,
    Driver,
    Load,
}

pub fn trace_signal(ws: &mut Workspace, p: &Json) -> Json {
    let Some(uri) = p.at(&["textDocument", "uri"]).as_str() else {
        return Json::Null;
    };
    let path = uri_to_path(uri);
    let f = match ws.file_id(&path) {
        Some(f) => f,
        None => match std::fs::read_to_string(&path) {
            Ok(t) => ws.update(&path, t),
            Err(_) => return Json::Null,
        },
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

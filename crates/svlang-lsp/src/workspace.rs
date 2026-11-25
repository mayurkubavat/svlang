//! All indexed files, a name table over them, and name resolution:
//! scopes, class inheritance, `pkg::`/`Class::` and `obj.member` chains.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::index::{FileIndex, SymKind, Symbol};
use crate::text::{is_ident_byte, word_at};

/// A symbol: file id and index in that file.
pub type SymRef = (usize, u32);

#[derive(Default)]
pub struct Workspace {
    pub files: Vec<Option<FileIndex>>,
    pub file_roots: Vec<Option<PathBuf>>,
    pub by_path: HashMap<PathBuf, usize>,
    /// File basename (e.g. `"my_item.svh"`) -> file ids with that basename.
    pub by_filename: HashMap<String, Vec<usize>>,
    /// Name -> every non-local symbol with that name.
    pub globals: HashMap<String, Vec<SymRef>>,
    /// Configured library roots (`libraryPaths`, `$UVM_HOME/src`).
    pub libraries: Vec<PathBuf>,
    /// Directories / project roots already quick-warmed on demand.
    warmed_roots: HashSet<PathBuf>,
    /// Memoized directory -> nearest ancestor git / `.svlang-lint` root.
    dir_root_cache: HashMap<PathBuf, Option<PathBuf>>,
}

pub const SV_EXTENSIONS: &[&str] = &["sv", "svh", "v", "vh", "svi", "svp"];
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "out",
    "dist",
    "build",
    "references",
    "corpus",
    "third_party",
    "vendor",
    "venv",
    "obj_dir",
    "csrc",
    "simv.daidir",
    "xcelium.d",
    "work",
    "informal_work",
];

pub fn is_skipped_dir_name(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name) || name.starts_with("build-") || name.starts_with("cmake-build-")
}

impl Workspace {
    pub fn sym(&self, r: SymRef) -> &Symbol {
        &self.files[r.0].as_ref().expect("live file").symbols[r.1 as usize]
    }

    pub fn file(&self, f: usize) -> &FileIndex {
        self.files[f].as_ref().expect("live file")
    }

    pub fn file_id(&self, path: &Path) -> Option<usize> {
        self.by_path.get(path).copied().filter(|&f| self.files[f].is_some())
    }

    pub fn project_root_for_path(&self, path: &Path) -> Option<PathBuf> {
        if let Some(f) = self.file_id(path) {
            if let Some(r) = self.file_roots.get(f).and_then(|r| r.clone()) {
                return Some(r);
            }
        }
        discover_project_root(path.parent().unwrap_or(path))
    }

    pub fn is_warmed(&self, root: &Path) -> bool {
        self.warmed_roots.contains(root)
    }

    pub fn mark_warmed(&mut self, root: PathBuf) {
        self.warmed_roots.insert(root);
    }

    fn resolve_dir_root(&mut self, dir: &Path) -> Option<PathBuf> {
        if let Some(cached) = self.dir_root_cache.get(dir) {
            return cached.clone();
        }
        let found = discover_project_root(dir);
        self.dir_root_cache.insert(dir.to_path_buf(), found.clone());
        found
    }

    fn unlink(&mut self, f: usize) {
        if let Some(old) = self.files[f].take() {
            if let Some(fname) = old.path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                if let Some(ids) = self.by_filename.get_mut(&fname) {
                    ids.retain(|&id| id != f);
                    if ids.is_empty() {
                        self.by_filename.remove(&fname);
                    }
                }
            }
            for s in old.symbols.iter().filter(|s| !s.local) {
                if let Some(v) = self.globals.get_mut(&s.name) {
                    v.retain(|r| r.0 != f);
                    if v.is_empty() {
                        self.globals.remove(&s.name);
                    }
                }
            }
        }
    }

    pub fn insert(&mut self, idx: FileIndex) -> usize {
        let proj_root = idx.path.parent().and_then(|d| self.resolve_dir_root(d));
        let f = match self.by_path.get(&idx.path) {
            Some(&f) => {
                self.unlink(f);
                self.file_roots[f] = proj_root;
                f
            }
            None => {
                self.files.push(None);
                self.file_roots.push(proj_root);
                let f = self.files.len() - 1;
                self.by_path.insert(idx.path.clone(), f);
                f
            }
        };
        if let Some(fname) = idx.path.file_name().map(|n| n.to_string_lossy().into_owned()) {
            let entry = self.by_filename.entry(fname).or_default();
            if !entry.contains(&f) {
                entry.push(f);
            }
        }
        for (i, s) in idx.symbols.iter().enumerate() {
            if !s.local {
                self.globals.entry(s.name.clone()).or_default().push((f, i as u32));
            }
        }
        self.files[f] = Some(idx);
        f
    }

    pub fn insert_batch(&mut self, built: Vec<FileIndex>) -> usize {
        let count = built.len();
        for idx in built {
            self.insert(idx);
        }
        count
    }

    pub fn remove(&mut self, path: &Path) {
        if let Some(&f) = self.by_path.get(path) {
            self.unlink(f);
        }
    }

    /// Collects SV files under `roots` without locking `Workspace`.
    pub fn collect_paths_lockfree(
        roots: &[PathBuf],
        exclude: &[String],
        limit: usize,
        deadline: Option<Instant>,
    ) -> (Vec<PathBuf>, bool) {
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        let mut truncated = false;
        for r in roots {
            if !collect(r, exclude, &mut paths, &mut seen, 0, limit, deadline, &mut truncated) {
                break;
            }
        }
        (paths, truncated)
    }

    /// Collects unindexed SV files under `roots`, optionally bounded by `limit` and `deadline`.
    pub fn collect_unindexed_bounded(
        &self,
        roots: &[PathBuf],
        exclude: &[String],
        limit: usize,
        deadline: Option<Instant>,
    ) -> (Vec<PathBuf>, bool) {
        let (mut paths, truncated) = Self::collect_paths_lockfree(roots, exclude, limit, deadline);
        paths.retain(|p| !self.by_path.contains_key(p));
        (paths, truncated)
    }

    /// Reads and parses `paths` into `FileIndex` values in parallel without touching `Workspace`.
    pub fn build_batch(paths: &[PathBuf]) -> Vec<FileIndex> {
        if paths.is_empty() {
            return Vec::new();
        }
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
        let chunk = paths.len().div_ceil(n.max(1)).max(1);
        std::thread::scope(|s| {
            let handles: Vec<_> = paths
                .chunks(chunk)
                .map(|ps| {
                    s.spawn(move || {
                        ps.iter()
                            .filter_map(|p| {
                                let bytes = std::fs::read(p).ok()?;
                                if bytes.len() > 8 << 20 {
                                    return None;
                                }
                                let text = String::from_utf8_lossy(&bytes).into_owned();
                                // A parser bug must not take the server down.
                                std::panic::catch_unwind(|| FileIndex::build(p, text)).ok()
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        })
    }

    /// Indexes every SV file under the roots, in parallel.
    pub fn scan(&mut self, roots: &[PathBuf], exclude: &[String]) -> usize {
        let (paths, _) = self.collect_unindexed_bounded(roots, exclude, 50_000, None);
        let built = Self::build_batch(&paths);
        self.insert_batch(built)
    }

    /// Ranks how close `cand_f` is to `from_f` (lower tuple = closer/better match).
    /// Tiers: 0 = same file, 1 = same directory, 2 = same git/project root,
    /// 3 = configured library (`$UVM_HOME/src`), 4 = other workspace directory.
    pub fn proximity(&self, from_f: usize, cand_f: usize) -> (u8, usize, u8) {
        if from_f == cand_f {
            return (0, 0, 0);
        }
        let from_p = &self.file(from_f).path;
        let cand_p = &self.file(cand_f).path;
        let testdata_pen = u8::from(!is_testdata_path(from_p) && is_testdata_path(cand_p));
        if from_p.parent() == cand_p.parent() {
            return (1, 0, testdata_pen);
        }
        let shared = shared_components(from_p, cand_p);
        let inv_shared = usize::MAX - shared;
        if let Some(Some(from_root)) = self.file_roots.get(from_f) {
            let cand_root = self.file_roots.get(cand_f).and_then(|r| r.as_ref());
            if cand_root == Some(from_root) || (cand_root.is_none() && cand_p.starts_with(from_root)) {
                return (2, inv_shared, testdata_pen);
            }
        }
        if self.libraries.iter().any(|lib| cand_p.starts_with(lib)) {
            return (3, inv_shared, testdata_pen);
        }
        (4, inv_shared, testdata_pen)
    }

    /// Resolves an `` `include "rel/or/base.svh" `` directive inside `from_f` to an indexed file id.
    fn resolve_include_file(&self, from_f: usize, inc: &str) -> Option<usize> {
        let inc_norm = inc.trim().replace('\\', "/");
        if inc_norm.is_empty() {
            return None;
        }
        let from_path = &self.file(from_f).path;
        if let Some(dir) = from_path.parent() {
            let rel = dir.join(&inc_norm);
            if let Some(id) = self.file_id(&rel) {
                return Some(id);
            }
            if let Ok(canon) = rel.canonicalize() {
                if let Some(id) = self.file_id(&canon) {
                    return Some(id);
                }
            }
        }
        let inc_path = Path::new(&inc_norm);
        let base = inc_path.file_name()?.to_str()?;
        let cands = self.by_filename.get(base)?;
        cands
            .iter()
            .copied()
            .filter(|&id| self.files[id].is_some() && self.file(id).path.ends_with(inc_path))
            .min_by_key(|&id| self.proximity(from_f, id))
    }

    // ---- resolution ----

    fn named(&self, name: &str) -> &[SymRef] {
        self.globals.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn best_global_near(&self, from_f: usize, name: &str) -> Option<SymRef> {
        self.named(name).iter().copied().min_by_key(|&r| {
            let s = self.sym(r);
            (
                !s.kind.is_type(),
                s.parent.is_some() && !matches!(self.parent_of(r).map(|p| self.sym(p).kind), Some(SymKind::Package)),
                self.proximity(from_f, r.0),
            )
        })
    }

    fn parent_of(&self, r: SymRef) -> Option<SymRef> {
        self.sym(r).parent.map(|p| (r.0, p))
    }

    /// Classes (and other scopes) called `name`; a typedef to one is followed.
    pub fn scopes_named(&self, name: &str, kinds: &[SymKind]) -> Vec<SymRef> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut todo = vec![(name.to_string(), 0)];
        while let Some((n, depth)) = todo.pop() {
            if depth > 4 || !seen.insert(n.clone()) {
                continue;
            }
            for &r in self.named(&n) {
                let s = self.sym(r);
                if kinds.contains(&s.kind) {
                    out.push(r);
                } else if s.kind == SymKind::Typedef {
                    if let Some(t) = &s.type_name {
                        todo.push((t.clone(), depth + 1));
                    }
                }
            }
        }
        out
    }

    /// Like `scopes_named`, ranked by path/project proximity to `from_f`.
    pub fn scopes_named_near(&self, from_f: usize, name: &str, kinds: &[SymKind]) -> Vec<SymRef> {
        let mut out = self.scopes_named(name, kinds);
        out.sort_by_key(|&r| self.proximity(from_f, r.0));
        out
    }

    /// Direct members of a scope, then `` `include ``d header items (for packages/units)
    /// and inherited base-class members (for classes).
    pub fn members(&self, scope: SymRef) -> Vec<SymRef> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.members_into(scope, &mut out, &mut seen, 0);
        out
    }

    fn collect_included_top_items(
        &self,
        inc_f: usize,
        out: &mut Vec<SymRef>,
        seen: &mut HashSet<SymRef>,
        depth: usize,
    ) {
        if depth > 24 || !seen.insert((inc_f, u32::MAX)) {
            return;
        }
        let inc_file = self.file(inc_f);
        for (i, s) in inc_file.children(None) {
            if !s.local && s.kind != SymKind::Macro {
                out.push((inc_f, i));
                if s.kind == SymKind::Typedef {
                    out.extend(
                        inc_file
                            .children(Some(i))
                            .filter(|(_, e)| e.kind == SymKind::EnumMember)
                            .map(|(j, _)| (inc_f, j)),
                    );
                }
            }
        }
        let nested: Vec<String> =
            inc_file.includes.iter().filter(|(enc, _)| enc.is_none()).map(|(_, p)| p.clone()).collect();
        for sub_inc in nested {
            if let Some(sub_f) = self.resolve_include_file(inc_f, &sub_inc) {
                self.collect_included_top_items(sub_f, out, seen, depth + 1);
            }
        }
    }

    fn members_into(&self, scope: SymRef, out: &mut Vec<SymRef>, seen: &mut HashSet<SymRef>, depth: usize) {
        if depth > 24 || !seen.insert(scope) {
            return;
        }
        let file = self.file(scope.0);
        for (i, s) in file.children(Some(scope.1)) {
            if !s.local {
                out.push((scope.0, i));
                // Enum values are visible in the scope holding the typedef.
                if s.kind == SymKind::Typedef {
                    out.extend(
                        file.children(Some(i))
                            .filter(|(_, e)| e.kind == SymKind::EnumMember)
                            .map(|(j, _)| (scope.0, j)),
                    );
                }
            }
        }
        let scoped_includes: Vec<String> =
            file.includes.iter().filter(|(enc, _)| *enc == Some(scope.1)).map(|(_, p)| p.clone()).collect();
        for inc in scoped_includes {
            if let Some(inc_f) = self.resolve_include_file(scope.0, &inc) {
                self.collect_included_top_items(inc_f, out, seen, depth + 1);
            }
        }
        let s = self.sym(scope);
        if s.kind == SymKind::Class {
            if let Some(mut base) = s.type_name.clone() {
                if let Some((_, param_sym)) = self
                    .file(scope.0)
                    .children(Some(scope.1))
                    .find(|(_, ps)| ps.kind == SymKind::Parameter && ps.name == base)
                {
                    if let Some(def_ty) = &param_sym.type_name {
                        base = def_ty.clone();
                    }
                }
                for b in self.scopes_named_near(scope.0, &base, &[SymKind::Class]).into_iter().take(1) {
                    self.members_into(b, out, seen, depth + 1);
                }
            }
        }
    }

    /// Out-of-block bodies `function T C::name` for a class member.
    pub fn bodies_of(&self, class: &str, name: &str) -> Vec<SymRef> {
        self.named(name).iter().copied().filter(|&r| self.sym(r).scope_class.as_deref() == Some(class)).collect()
    }

    /// Out-of-block bodies ranked by proximity to `from_f`.
    pub fn bodies_of_near(&self, from_f: usize, class: &str, name: &str) -> Vec<SymRef> {
        let mut v = self.bodies_of(class, name);
        v.sort_by_key(|&r| self.proximity(from_f, r.0));
        v
    }

    /// The class a method body belongs to: its parent, or `C` of `C::f`.
    fn class_of_method(&self, m: SymRef) -> Option<SymRef> {
        let s = self.sym(m);
        if let Some(c) = &s.scope_class {
            return self.scopes_named_near(m.0, c, &[SymKind::Class]).first().copied();
        }
        let p = self.parent_of(m)?;
        (self.sym(p).kind == SymKind::Class).then_some(p)
    }

    /// The scopes around `offset`, innermost first, with the class of an
    /// out-of-block method put after the method.
    pub fn scope_chain(&self, f: usize, offset: u32) -> Vec<SymRef> {
        let file = self.file(f);
        let mut out = Vec::new();
        for i in file.enclosing(offset).into_iter().rev() {
            let r = (f, i);
            out.push(r);
            if self.sym(r).scope_class.is_some() {
                if let Some(c) = self.class_of_method(r) {
                    out.push(c);
                }
            }
        }
        out
    }

    pub fn enclosing_class(&self, f: usize, offset: u32) -> Option<SymRef> {
        self.scope_chain(f, offset).into_iter().find(|&r| self.sym(r).kind == SymKind::Class)
    }

    /// Everything visible by plain name at `offset`, innermost first
    /// (locals, the scope's members with inherited ones, file level,
    /// imported packages). Names repeat when shadowed.
    pub fn visible(&self, f: usize, offset: u32) -> Vec<SymRef> {
        let mut out = Vec::new();
        let file = self.file(f);
        for scope in self.scope_chain(f, offset) {
            if scope.0 == f
                && matches!(
                    self.sym(scope).kind,
                    SymKind::Function | SymKind::Task | SymKind::Covergroup | SymKind::Clocking
                )
            {
                // Ports and locals declared before the cursor.
                for (i, s) in file.children(Some(scope.1)) {
                    if s.start <= offset || s.kind == SymKind::Port {
                        out.push((f, i));
                    }
                }
            } else {
                out.extend(self.members(scope));
            }
        }
        for (i, s) in file.children(None) {
            out.push((f, i));
            if s.kind == SymKind::Typedef {
                out.extend(file.children(Some(i)).map(|(j, _)| (f, j)));
            }
        }
        for pkg in &file.imports {
            for p in self.scopes_named_near(f, pkg, &[SymKind::Package]).into_iter().take(1) {
                out.extend(self.members(p));
            }
        }
        out
    }

    /// The scope a symbol's value or type leads to (for `x.` / `X::`).
    pub fn target_of(&self, r: SymRef) -> Option<SymRef> {
        let s = self.sym(r);
        match s.kind {
            SymKind::Class
            | SymKind::Package
            | SymKind::Module
            | SymKind::Interface
            | SymKind::Program
            | SymKind::Checker => Some(r),
            SymKind::Typedef => self.scopes_named_near(r.0, &s.name, &[SymKind::Class]).first().copied(),
            SymKind::Variable | SymKind::Port | SymKind::Parameter | SymKind::Instance | SymKind::Function => {
                let t = s.type_name.as_ref()?;
                self.scopes_named_near(r.0, t, &[SymKind::Class, SymKind::Module, SymKind::Interface, SymKind::Program])
                    .first()
                    .copied()
            }
            _ => None,
        }
    }

    /// Resolves a chain of names (`a`, `.b`, `::c`) to the scope its last
    /// part leads to.
    pub fn resolve_chain(&self, f: usize, offset: u32, chain: &[(Sep, String)], trailing: Sep) -> Option<SymRef> {
        let mut cur: Option<SymRef> = None;
        for (k, (sep, name)) in chain.iter().enumerate() {
            let next_sep = chain.get(k + 1).map(|c| c.0).or(Some(trailing));
            cur = match cur {
                None => match name.as_str() {
                    "this" => self.enclosing_class(f, offset),
                    "super" => {
                        let c = self.enclosing_class(f, offset)?;
                        let base = self.sym(c).type_name.clone()?;
                        self.scopes_named_near(f, &base, &[SymKind::Class]).first().copied()
                    }
                    _ if next_sep == Some(Sep::Scope) && *sep == Sep::None => {
                        let mut s = self.scopes_named_near(f, name, &[SymKind::Package, SymKind::Class]);
                        if s.is_empty() {
                            s = self
                                .visible(f, offset)
                                .into_iter()
                                .filter(|&r| self.sym(r).name == *name)
                                .filter_map(|r| self.target_of(r))
                                .collect();
                        }
                        s.first().copied()
                    }
                    _ => {
                        let r =
                            self.visible(f, offset).into_iter().find(|&r| self.sym(r).name == *name).or_else(|| {
                                let mut cands: Vec<SymRef> =
                                    self.named(name).iter().copied().filter(|&r| self.target_of(r).is_some()).collect();
                                cands.sort_by_key(|&r| self.proximity(f, r.0));
                                cands.into_iter().next()
                            })?;
                        self.target_of(r)
                    }
                },
                Some(scope) => {
                    if name == "type_id" || name == "super" {
                        Some(scope)
                    } else {
                        let m = self.members(scope).into_iter().find(|&r| self.sym(r).name == *name)?;
                        self.target_of(m)
                    }
                }
            };
            cur?;
        }
        cur
    }

    fn valid_type_clause_target(&self, r: SymRef, self_cls: Option<SymRef>) -> bool {
        if Some(r) == self_cls {
            return false;
        }
        let s = self.sym(r);
        s.kind.is_type()
            || (s.kind == SymKind::Parameter
                && (s.type_name.is_some() || s.detail.split_whitespace().any(|w| w == "type")))
    }

    /// The declarations the name at `offset` may refer to, best first.
    pub fn definitions(&self, f: usize, offset: u32) -> Vec<SymRef> {
        let file = self.file(f);
        let Some((s, e)) = word_at(&file.text, offset) else {
            return Vec::new();
        };
        let word = &file.text[s..e];
        if let Some(m) = word.strip_prefix('`') {
            let mut macros: Vec<SymRef> =
                self.named(m).iter().copied().filter(|&r| self.sym(r).kind == SymKind::Macro).collect();
            macros.sort_by_key(|&r| self.proximity(f, r.0));
            return macros;
        }
        // On a declaration's own name: that declaration, plus the other
        // half of an extern/out-of-block method pair.
        if let Some(i) = file.symbols.iter().position(|sy| sy.sel_start as usize == s && sy.sel_end as usize == e) {
            let me = (f, i as u32);
            let sy = self.sym(me);
            let mut out = vec![me];
            if let Some(c) = &sy.scope_class {
                for cl in self.scopes_named_near(f, c, &[SymKind::Class]).into_iter().take(2) {
                    out.extend(
                        self.members(cl).into_iter().filter(|&r| {
                            self.sym(r).name == sy.name && r.0 == cl.0 && self.sym(r).parent == Some(cl.1)
                        }),
                    );
                }
            } else if let Some(p) = self.parent_of(me).filter(|&p| self.sym(p).kind == SymKind::Class) {
                out.extend(self.bodies_of_near(f, &self.sym(p).name, &sy.name));
            }
            return out;
        }

        let chain = chain_before(&file.text, s);
        let chain_start = chain_start_offset(&file.text, s);
        let in_type_clause = is_type_clause_before(&file.text, chain_start);
        let self_cls = if in_type_clause { enclosing_header_class(file, f, s) } else { None };

        if !chain.is_empty() {
            let sep = chain_sep(&file.text, s);
            if let Some(scope) = self.resolve_chain(f, offset, &chain, sep) {
                let mut hits: Vec<SymRef> = self
                    .members(scope)
                    .into_iter()
                    .filter(|&r| {
                        self.sym(r).name == word && (!in_type_clause || self.valid_type_clause_target(r, self_cls))
                    })
                    .collect();
                let extra: Vec<SymRef> = hits
                    .iter()
                    .filter_map(|&r| self.parent_of(r).map(|p| (p, self.sym(r).name.clone())))
                    .flat_map(|(p, n)| self.bodies_of_near(f, &self.sym(p).name.clone(), &n))
                    .collect();
                hits.extend(extra);
                if !hits.is_empty() {
                    return hits;
                }
            }
            // Unresolved receiver: any class/scope member of that name (or top-level type for `pkg::B`).
            let mut members: Vec<SymRef> = self
                .named(word)
                .iter()
                .copied()
                .filter(|&r| {
                    if in_type_clause && !self.valid_type_clause_target(r, self_cls) {
                        return false;
                    }
                    self.parent_of(r).is_some_and(|p| self.sym(p).kind.is_scope())
                        || self.sym(r).scope_class.is_some()
                        || (sep == Sep::Scope && self.sym(r).kind.is_type())
                })
                .collect();
            members.sort_by_key(|&r| self.proximity(f, r.0));
            if !members.is_empty() {
                return members;
            }
        }

        let vis: Vec<SymRef> = self
            .visible(f, offset)
            .into_iter()
            .filter(|&r| self.sym(r).name == word && (!in_type_clause || self.valid_type_clause_target(r, self_cls)))
            .collect();
        if let Some(&first) = vis.first() {
            let mut out = vec![first];
            if let Some(p) = self.parent_of(first).filter(|&p| self.sym(p).kind == SymKind::Class) {
                out.extend(self.bodies_of_near(f, &self.sym(p).name, word));
            }
            if in_type_clause && self.sym(first).kind == SymKind::Parameter {
                if let Some(def_ty) = &self.sym(first).type_name {
                    out.extend(
                        self.scopes_named_near(f, def_ty, &[SymKind::Class, SymKind::Interface, SymKind::Typedef])
                            .into_iter()
                            .take(1),
                    );
                }
            }
            return out;
        }

        // Anything else of that name; types and units in the closest repo/dir first.
        let mut all: Vec<SymRef> = self
            .named(word)
            .iter()
            .copied()
            .filter(|&r| !in_type_clause || self.valid_type_clause_target(r, self_cls))
            .collect();
        if all.is_empty() && in_type_clause {
            all = self.named(word).iter().copied().filter(|&r| Some(r) != self_cls).collect();
        }
        all.sort_by_key(|&r| {
            let s = self.sym(r);
            (
                !s.kind.is_type(),
                s.parent.is_some() && !matches!(self.parent_of(r).map(|p| self.sym(p).kind), Some(SymKind::Package)),
                self.proximity(f, r.0),
            )
        });
        all
    }
}

fn discover_project_root(start_dir: &Path) -> Option<PathBuf> {
    for anc in start_dir.ancestors() {
        if anc.join(".git").exists() || anc.join(".svlang-lint").exists() || anc.join("svlang_lint_config.py").exists()
        {
            return Some(anc.to_path_buf());
        }
    }
    None
}

fn is_testdata_path(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == "testdata")
}

fn shared_components(a: &Path, b: &Path) -> usize {
    a.components().zip(b.components()).take_while(|(x, y)| x == y).count()
}

/// SV files under `dir`, skipping hidden, build-output, corpus, and excluded
/// directories (an exclude matches a directory name or a path suffix).
#[allow(clippy::too_many_arguments)]
fn collect(
    dir: &Path,
    exclude: &[String],
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    depth: usize,
    limit: usize,
    deadline: Option<Instant>,
    truncated: &mut bool,
) -> bool {
    if out.len() >= limit {
        *truncated = true;
        return false;
    }
    if let Some(dl) = deadline {
        if Instant::now() >= dl {
            *truncated = true;
            return false;
        }
    }
    if depth > 32 {
        return true;
    }
    let Ok(canon) = dir.canonicalize() else {
        return true;
    };
    if !seen.insert(canon) {
        return true;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return true;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= limit {
            *truncated = true;
            return false;
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                *truncated = true;
                return false;
            }
        }
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir() || (ft.is_symlink() && p.is_dir());
        if is_dir {
            if is_skipped_dir_name(&name) {
                continue;
            }
            let ps = p.to_string_lossy();
            if exclude.iter().any(|x| !x.is_empty() && (name == *x || ps.ends_with(x.as_str()))) {
                continue;
            }
            if !collect(&p, exclude, out, seen, depth + 1, limit, deadline, truncated) {
                return false;
            }
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| SV_EXTENSIONS.contains(&x)) {
            out.push(p);
        }
    }
    true
}

/// Byte offset of the start of the receiver chain preceding `s` (or `s` itself).
fn chain_start_offset(text: &str, s: usize) -> usize {
    let b = text.as_bytes();
    let skip_ws = |mut j: usize| {
        while j > 0 && matches!(b[j - 1], b' ' | b'\t') {
            j -= 1;
        }
        j
    };
    let mut i = s;
    loop {
        let mut j = skip_ws(i);
        if j >= 2 && &b[j - 2..j] == b"::" {
            j -= 2;
        } else if j >= 1 && b[j - 1] == b'.' {
            j -= 1;
        } else {
            break;
        }
        let mut k = skip_ws(j);
        while k > 0 && matches!(b[k - 1], b')' | b']') {
            let (open, close) = if b[k - 1] == b')' { (b'(', b')') } else { (b'[', b']') };
            let mut depth = 0i32;
            let mut m = k;
            while m > 0 {
                m -= 1;
                if b[m] == close {
                    depth += 1;
                } else if b[m] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            k = skip_ws(m);
            if k > 0 && b[k - 1] == b'#' {
                k = skip_ws(k - 1);
            }
        }
        let end = k;
        while k > 0 && is_ident_byte(b[k - 1]) {
            k -= 1;
        }
        if k == end {
            break;
        }
        i = k;
    }
    i
}

/// Returns true if byte `pos` sits right after `extends` or `implements` (including
/// comma-separated interface lists `implements I1, I2`).
fn is_type_clause_before(text: &str, pos: usize) -> bool {
    let b = text.as_bytes();
    let mut i = pos.min(b.len());
    let mut expect_prior_type = false;
    loop {
        while i > 0 && matches!(b[i - 1], b' ' | b'\t' | b'\r' | b'\n') {
            i -= 1;
        }
        if expect_prior_type && i > 0 && b[i - 1] == b')' {
            let mut depth = 0i32;
            while i > 0 {
                i -= 1;
                if b[i] == b')' {
                    depth += 1;
                } else if b[i] == b'(' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            while i > 0 && matches!(b[i - 1], b' ' | b'\t' | b'#') {
                i -= 1;
            }
        }
        let end = i;
        while i > 0 && (is_ident_byte(b[i - 1]) || b[i - 1] == b':') {
            i -= 1;
        }
        if i < end {
            let kw = &text[i..end];
            if !expect_prior_type && matches!(kw, "extends" | "implements") {
                return true;
            }
            if expect_prior_type {
                expect_prior_type = false;
                continue;
            }
        } else if !expect_prior_type && i > 0 && b[i - 1] == b',' {
            i -= 1;
            expect_prior_type = true;
            continue;
        }
        return false;
    }
}

/// If byte `s` lies in the `extends`/`implements` header of an enclosing class,
/// returns that class so `class Foo extends pkg::Foo` never resolves to itself.
fn enclosing_header_class(file: &FileIndex, f: usize, s: usize) -> Option<SymRef> {
    let off = s as u32;
    for i in file.enclosing(off).into_iter().rev() {
        let sym = &file.symbols[i as usize];
        if sym.kind == SymKind::Class && s >= sym.sel_end as usize {
            let between = &file.text[sym.sel_end as usize..s.min(file.text.len())];
            let mut depth = 0i32;
            let mut seen_header_semi = false;
            for ch in between.bytes() {
                match ch {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b';' if depth <= 0 => {
                        seen_header_semi = true;
                        break;
                    }
                    _ => {}
                }
            }
            if !seen_header_semi {
                return Some((f, i));
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sep {
    None,
    Dot,
    Scope,
}

/// The separator right before byte `s` (skipping spaces): `.` or `::`.
pub fn chain_sep(text: &str, s: usize) -> Sep {
    let b = text.as_bytes();
    let mut i = s;
    while i > 0 && b[i - 1] == b' ' {
        i -= 1;
    }
    if i >= 2 && &b[i - 2..i] == b"::" {
        Sep::Scope
    } else if i >= 1 && b[i - 1] == b'.' {
        Sep::Dot
    } else {
        Sep::None
    }
}

/// The receiver chain before byte `s`: for `a.b().c::d|` gives
/// `[(None,a), (Dot,b), (Dot,c)]`, each part with the separator before it
/// (the one before `s` is `chain_sep`). Empty when nothing precedes.
pub fn chain_before(text: &str, s: usize) -> Vec<(Sep, String)> {
    let b = text.as_bytes();
    let skip_ws = |mut j: usize| {
        while j > 0 && matches!(b[j - 1], b' ' | b'\t') {
            j -= 1;
        }
        j
    };
    // Parts right to left, each with the separator after it.
    let mut parts: Vec<(String, Sep)> = Vec::new();
    let mut i = s;
    loop {
        let mut j = skip_ws(i);
        let sep = if j >= 2 && &b[j - 2..j] == b"::" {
            j -= 2;
            Sep::Scope
        } else if j >= 1 && b[j - 1] == b'.' {
            j -= 1;
            Sep::Dot
        } else {
            break;
        };
        let mut k = skip_ws(j);
        // Skip a call's parentheses, an index or a parameter list.
        while k > 0 && matches!(b[k - 1], b')' | b']') {
            let (open, close) = if b[k - 1] == b')' { (b'(', b')') } else { (b'[', b']') };
            let mut depth = 0;
            let mut m = k;
            while m > 0 {
                m -= 1;
                if b[m] == close {
                    depth += 1;
                } else if b[m] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            k = skip_ws(m);
            if k > 0 && b[k - 1] == b'#' {
                k = skip_ws(k - 1);
            }
        }
        let end = k;
        while k > 0 && is_ident_byte(b[k - 1]) {
            k -= 1;
        }
        if k == end {
            break;
        }
        parts.push((text[k..end].to_string(), sep));
        i = k;
    }
    parts.reverse();
    let mut out = Vec::with_capacity(parts.len());
    let mut before = Sep::None;
    for (name, after) in parts {
        out.push((before, name));
        before = after;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains() {
        let t = "x = a.b().c::d";
        let s = t.len() - 1;
        let c = chain_before(t, s);
        let names: Vec<&str> = c.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        let seps: Vec<Sep> = c.iter().map(|(s, _)| *s).collect();
        assert_eq!(seps, vec![Sep::None, Sep::Dot, Sep::Dot]);
        assert_eq!(chain_sep(t, s), Sep::Scope);
        assert!(chain_before("foo", 0).is_empty());
        let t2 = "p::c::f";
        let c2 = chain_before(t2, 6);
        assert_eq!(c2, vec![(Sep::None, "p".to_string()), (Sep::Scope, "c".to_string())]);
        assert!(is_type_clause_before("class A extends B;", 16));
        assert!(is_type_clause_before("class A implements I1 #(8), pkg::I2;", 28));
        assert!(!is_type_clause_before("class A; B x;", 9));
    }
}

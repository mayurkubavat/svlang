//! The LSP side: message framing on stdio, document sync, and the
//! requests (symbols, definition, references, hover, completion).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::index::{FileIndex, SymKind, Symbol};
use crate::json::{self, obj, Json};
use crate::text::{path_to_uri, uri_to_path, word_at};
use crate::workspace::{chain_before, chain_sep, Sep, SymRef, Workspace};

pub struct Server {
    pub ws: Arc<RwLock<Workspace>>,
    roots: Vec<PathBuf>,
    libraries: Vec<PathBuf>,
    exclude: Vec<String>,
    open: HashSet<PathBuf>,
    shutdown: bool,
    indexing: Arc<AtomicBool>,
    cancel_index: Arc<AtomicBool>,
    out: crate::lint::Out,
    /// Diagnostics and quick fixes; None when the editor turned them off.
    linter: Option<crate::lint::Linter>,
}

/// Reads one `Content-Length` framed message; None at end of input.
pub fn read_message(r: &mut impl BufRead) -> Option<String> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let mut buf = vec![0u8; len?];
    r.read_exact(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

impl Server {
    pub fn new(out: Box<dyn Write + Send>) -> Self {
        Server {
            ws: Arc::new(RwLock::new(Workspace::default())),
            roots: Vec::new(),
            libraries: Vec::new(),
            exclude: Vec::new(),
            open: HashSet::new(),
            shutdown: false,
            indexing: Arc::new(AtomicBool::new(false)),
            cancel_index: Arc::new(AtomicBool::new(false)),
            out: Arc::new(std::sync::Mutex::new(out)),
            linter: None,
        }
    }

    fn send(&mut self, msg: Json) {
        crate::lint::send(&self.out, &msg);
    }

    fn reply(&mut self, id: &Json, result: Json) {
        self.send(obj(vec![("jsonrpc", "2.0".into()), ("id", id.clone()), ("result", result)]));
    }

    fn notify(&mut self, method: &str, params: Json) {
        self.send(obj(vec![("jsonrpc", "2.0".into()), ("method", method.into()), ("params", params)]));
    }

    fn log(&mut self, msg: &str) {
        self.notify("window/logMessage", obj(vec![("type", 4u32.into()), ("message", msg.into())]));
    }

    /// Runs until `exit`; returns the process exit code.
    pub fn run(&mut self, input: &mut impl BufRead) -> i32 {
        while let Some(raw) = read_message(input) {
            let Ok(msg) = json::parse(&raw) else { continue };
            let method = msg.get("method").as_str().unwrap_or("").to_string();
            if method == "exit" {
                return if self.shutdown { 0 } else { 1 };
            }
            let id = msg.get("id").clone();
            let params = msg.get("params");
            let t0 = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle(&method, params)));
            match result {
                Ok(Some(r)) if !id.is_null() => self.reply(&id, r),
                Ok(None) if !id.is_null() && !method.is_empty() => {
                    self.send(obj(vec![
                        ("jsonrpc", "2.0".into()),
                        ("id", id),
                        (
                            "error",
                            obj(vec![
                                ("code", Json::Num(-32601.0)),
                                ("message", format!("unhandled method {}", method).into()),
                            ]),
                        ),
                    ]));
                }
                Err(_) if !id.is_null() => {
                    self.send(obj(vec![
                        ("jsonrpc", "2.0".into()),
                        ("id", id),
                        ("error", obj(vec![("code", Json::Num(-32603.0)), ("message", "internal error".into())])),
                    ]));
                }
                _ => {}
            }
            let ms = t0.elapsed().as_millis();
            if ms > 200 {
                eprintln!("[svlang-lsp] {} took {} ms", method, ms);
            }
        }
        1
    }

    /// Answers a request (Some) or handles a notification (None).
    pub fn handle(&mut self, method: &str, p: &Json) -> Option<Json> {
        match method {
            "initialize" => Some(self.initialize(p)),
            "initialized" => {
                self.index_workspace();
                None
            }
            "shutdown" => {
                self.shutdown = true;
                self.cancel_index.store(true, Ordering::SeqCst);
                Some(Json::Null)
            }
            "textDocument/didOpen" => {
                let uri = p.at(&["textDocument", "uri"]).as_str()?;
                let path = uri_to_path(uri);
                let text = p.at(&["textDocument", "text"]).as_str()?.to_string();
                self.open.insert(path.clone());
                if let Some(l) = &self.linter {
                    l.opened(&path, uri, &text);
                }
                let built = FileIndex::build(&path, text);
                self.ws.write().unwrap_or_else(|e| e.into_inner()).insert(built);
                self.warm_subproject_async(&path);
                None
            }
            "textDocument/didChange" => {
                let uri = p.at(&["textDocument", "uri"]).as_str()?;
                let path = uri_to_path(uri);
                // Full sync: the last change holds the whole text.
                let text = p.get("contentChanges").as_arr().last()?.get("text").as_str()?.to_string();
                if let Some(l) = &self.linter {
                    l.changed(&path, uri, &text);
                }
                let built = FileIndex::build(&path, text);
                self.ws.write().unwrap_or_else(|e| e.into_inner()).insert(built);
                self.warm_subproject_async(&path);
                None
            }
            "textDocument/didClose" => {
                let uri = p.at(&["textDocument", "uri"]).as_str()?;
                let path = uri_to_path(uri);
                self.open.remove(&path);
                if let Some(l) = &self.linter {
                    l.closed(&path, uri);
                }
                match std::fs::read_to_string(&path) {
                    Ok(t) if self.in_scope(&path) => {
                        let built = FileIndex::build(&path, t);
                        self.ws.write().unwrap_or_else(|e| e.into_inner()).insert(built);
                    }
                    _ => self.ws.write().unwrap_or_else(|e| e.into_inner()).remove(&path),
                }
                None
            }
            "textDocument/didSave" => {
                // A saved file may be one other open files include.
                if let Some(l) = &self.linter {
                    l.saved();
                }
                None
            }
            "textDocument/codeAction" => {
                let uri = p.at(&["textDocument", "uri"]).as_str()?;
                let first = p.at(&["range", "start", "line"]).as_u32()?;
                let last = p.at(&["range", "end", "line"]).as_u32().unwrap_or(first);
                let only: Vec<String> =
                    p.at(&["context", "only"]).as_arr().iter().filter_map(|v| v.as_str().map(String::from)).collect();
                Some(match &self.linter {
                    Some(l) => l.code_actions(&uri_to_path(uri), uri, first, last, &only),
                    None => Json::Arr(Vec::new()),
                })
            }
            "workspace/didChangeWatchedFiles" => {
                // A source, a config or a waiver file changed on disk.
                if let Some(l) = &self.linter {
                    l.request();
                }
                let mut updates: Vec<FileIndex> = Vec::new();
                let mut removals: Vec<PathBuf> = Vec::new();
                for ch in p.get("changes").as_arr() {
                    let Some(uri) = ch.get("uri").as_str() else {
                        continue;
                    };
                    let path = uri_to_path(uri);
                    if self.open.contains(&path) {
                        continue;
                    }
                    match ch.get("type").as_u32() {
                        Some(3) => removals.push(path),
                        _ => {
                            if let Ok(t) = std::fs::read_to_string(&path) {
                                updates.push(FileIndex::build(&path, t));
                            }
                        }
                    }
                }
                if !updates.is_empty() || !removals.is_empty() {
                    let mut ws = self.ws.write().unwrap_or_else(|e| e.into_inner());
                    for path in removals {
                        ws.remove(&path);
                    }
                    ws.insert_batch(updates);
                }
                None
            }
            "textDocument/documentSymbol" => Some(self.document_symbols(p)),
            "workspace/symbol" => Some(self.workspace_symbols(p)),
            "textDocument/definition" | "textDocument/declaration" => Some(self.definition(p)),
            "textDocument/typeDefinition" => Some(self.type_definition(p)),
            "textDocument/references" => Some(self.references(p)),
            "textDocument/documentHighlight" => Some(self.highlights(p)),
            "textDocument/hover" => Some(self.hover(p)),
            "textDocument/completion" => Some(self.completion(p)),
            "textDocument/semanticTokens/full" => {
                let uri = p.at(&["textDocument", "uri"]).as_str().unwrap_or("");
                if !uri.is_empty() {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::semantic_tokens::semantic_tokens_full(&ws, uri))
            }
            "textDocument/prepareTypeHierarchy" => {
                if let Some(uri) = p.at(&["textDocument", "uri"]).as_str() {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::prepare_type_hierarchy(&ws, p))
            }
            "typeHierarchy/supertypes" => {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::supertypes(&ws, p))
            }
            "typeHierarchy/subtypes" => {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::subtypes(&ws, p))
            }
            "textDocument/prepareCallHierarchy" => {
                if let Some(uri) = p.at(&["textDocument", "uri"]).as_str() {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::prepare_call_hierarchy(&ws, p))
            }
            "callHierarchy/incomingCalls" => {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::incoming_calls(&ws, p))
            }
            "callHierarchy/outgoingCalls" => {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::hierarchy::outgoing_calls(&ws, p))
            }
            "svlang/inactiveRanges" => {
                let uri = p.at(&["textDocument", "uri"]).as_str().unwrap_or("");
                if !uri.is_empty() {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let defs = self.active_defines();
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::semantic_tokens::inactive_ranges(&ws, uri, &defs))
            }
            "svlang/rtlHierarchy" => {
                if let Some(uri) = p.get("activeUri").as_str().filter(|s| !s.is_empty()) {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::rtl_trace::rtl_hierarchy(&ws, &self.libraries, p))
            }
            "svlang/traceSignal" => {
                if let Some(uri) = p.at(&["textDocument", "uri"]).as_str() {
                    let _ = self.ensure_file(&uri_to_path(uri));
                }
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                Some(crate::rtl_trace::trace_signal(&ws, p))
            }
            "svlang/lintNow" => {
                if let Some(l) = &self.linter {
                    l.lint_now();
                }
                Some(obj(vec![("ok", true.into())]))
            }
            "svlang/setLintOptions" => {
                if let Some(l) = &self.linter {
                    let trig = p.get("trigger").as_str().map(crate::lint::LintTrigger::from_str);
                    let mut ed = l.editor_options();
                    apply_editor_json(&mut ed, p);
                    l.set_options(trig, Some(ed));
                }
                Some(obj(vec![("ok", true.into())]))
            }
            "svlang/status" => Some(self.current_status()),
            _ => None,
        }
    }

    fn in_scope(&self, path: &std::path::Path) -> bool {
        self.roots.iter().chain(&self.libraries).any(|r| path.starts_with(r))
    }

    fn initialize(&mut self, p: &Json) -> Json {
        for f in p.get("workspaceFolders").as_arr() {
            if let Some(u) = f.get("uri").as_str() {
                self.roots.push(uri_to_path(u));
            }
        }
        if self.roots.is_empty() {
            if let Some(u) = p.get("rootUri").as_str() {
                self.roots.push(uri_to_path(u));
            } else if let Some(r) = p.get("rootPath").as_str() {
                self.roots.push(PathBuf::from(r));
            }
        }
        let opts = p.get("initializationOptions");
        for l in opts.get("libraryPaths").as_arr() {
            if let Some(s) = l.as_str() {
                self.libraries.push(PathBuf::from(expand_env(s)));
            }
        }
        // $UVM_HOME/src gives UVM's own classes (uvm_component, ...); fall back to
        // ~/.cache/svlang/uvm-core-*/src when GUI editors don't inherit shell UVM_HOME.
        if opts.get("useUvmHome").as_bool() != Some(false) {
            if let Some(src) = discover_uvm_src() {
                if !self.libraries.contains(&src) {
                    self.libraries.push(src);
                }
            }
        }
        self.ws.write().unwrap_or_else(|e| e.into_inner()).libraries = self.libraries.clone();
        for e in opts.get("exclude").as_arr() {
            if let Some(s) = e.as_str() {
                self.exclude.push(s.trim_matches('/').trim_start_matches("**/").trim_end_matches("/**").to_string());
            }
        }
        // Always start the background lint worker unless explicitly disabled with
        // `"lint": false` AND no trigger given, so `svlang/lintNow` and dynamic
        // `svlang/setLintOptions` work without restarting the language server.
        let lint_enabled = opts.get("lint").as_bool() != Some(false);
        let initial_trigger = if !lint_enabled {
            crate::lint::LintTrigger::Off
        } else if let Some(t) = opts.get("lintTrigger").as_str() {
            crate::lint::LintTrigger::from_str(t)
        } else {
            crate::lint::LintTrigger::OnType
        };
        let root = self.roots.first().cloned().or_else(|| std::env::current_dir().ok()).unwrap_or_default();
        let mut editor = svlang_lint::EditorOptions::default();
        apply_editor_json(&mut editor, opts);
        self.linter = Some(crate::lint::Linter::start_with_options(root, initial_trigger, editor, self.out.clone()));

        let sync = obj(vec![("openClose", true.into()), ("change", 1u32.into()), ("save", true.into())]);
        // Code browsing, unless the editor asks for lint only
        // (`"navigation": false`).
        let nav = opts.get("navigation").as_bool() != Some(false);
        let sem_hl = opts.get("semanticHighlighting").as_bool() != Some(false);
        let completion = obj(vec![
            ("triggerCharacters", Json::Arr(vec![".".into(), ":".into(), "`".into(), "$".into()])),
            ("resolveProvider", false.into()),
        ]);
        let mut caps = vec![("textDocumentSync", sync)];
        if nav {
            for c in [
                "documentSymbolProvider",
                "workspaceSymbolProvider",
                "definitionProvider",
                "declarationProvider",
                "typeDefinitionProvider",
                "referencesProvider",
                "documentHighlightProvider",
                "hoverProvider",
                "typeHierarchyProvider",
                "callHierarchyProvider",
            ] {
                caps.push((c, true.into()));
            }
            caps.push(("completionProvider", completion));
            if sem_hl {
                caps.push(("semanticTokensProvider", crate::semantic_tokens::capabilities()));
            }
        }
        caps.push((
            "codeActionProvider",
            obj(vec![(
                "codeActionKinds",
                Json::Arr(vec!["quickfix".into(), "source.fixAll".into(), "source.fixAll.svlang".into()]),
            )]),
        ));
        obj(vec![
            ("capabilities", obj(caps)),
            ("serverInfo", obj(vec![("name", "svlang".into()), ("version", env!("CARGO_PKG_VERSION").into())])),
        ])
    }

    fn active_defines(&self) -> Vec<String> {
        self.linter
            .as_ref()
            .map(|l| {
                l.editor_options().defines.into_iter().map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect()
            })
            .unwrap_or_default()
    }

    fn current_status(&self) -> Json {
        let (files, symbols) = {
            let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
            let files = ws.files.iter().flatten().count();
            let symbols: usize = ws.files.iter().flatten().map(|f| f.symbols.len()).sum();
            (files, symbols)
        };
        let mut st = match &self.linter {
            Some(l) => l.status(),
            None => crate::lint::LintStatus::default(),
        };
        if self.indexing.load(Ordering::Relaxed) {
            st.state = "indexing".into();
        }
        st.to_json(Some((files, symbols)))
    }

    fn index_workspace(&mut self) {
        let t0 = Instant::now();
        let exclude = self.exclude.clone();

        // Partition roots: real project roots (having `.git` / `.svlang-lint` / `svlang_lint_config.py`)
        // vs umbrella parent folders (e.g. `~/Silicon` holding many independent git clones).
        let mut eager_roots: Vec<PathBuf> = self.libraries.clone();
        let mut umbrella_roots: Vec<PathBuf> = Vec::new();
        for r in &self.roots {
            if is_explicit_project_root(r) {
                eager_roots.push(r.clone());
            } else {
                umbrella_roots.push(r.clone());
            }
        }

        // For non-git workspace folders: if total SV files <= 1,500, treat as a normal single project
        // and index eagerly; if > 1,500 (multi-repo parent directory), index subprojects on demand as
        // files are opened or navigated so unrelated clones don't burn CPU/RAM or pollute symbols.
        for u in umbrella_roots {
            let (probe, over_cap) = Workspace::collect_paths_lockfree(
                std::slice::from_ref(&u),
                &exclude,
                1_500,
                Some(Instant::now() + Duration::from_millis(40)),
            );
            if !over_cap {
                eager_roots.push(u);
            } else {
                let _ = probe;
                let msg = format!(
                    "svlang: multi-repo parent workspace detected at {}; indexing subprojects on demand",
                    u.display()
                );
                eprintln!("[{}]", msg);
                self.log(&msg);
            }
        }

        if eager_roots.is_empty() {
            let status = self.current_status();
            self.notify("svlang/status", status);
            return;
        }

        // Fast bounded scan (<= 300 files, <= 30 ms): finishes small/medium single-IP repos
        // deterministically in a single step while never blocking large workspaces.
        let (initial_paths, truncated) = Workspace::collect_paths_lockfree(
            &eager_roots,
            &exclude,
            300,
            Some(Instant::now() + Duration::from_millis(30)),
        );

        if !truncated {
            let to_build: Vec<PathBuf> = {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                initial_paths.into_iter().filter(|p| !ws.by_path.contains_key(p)).collect()
            };
            let built = Workspace::build_batch(&to_build);
            let (n, syms) = {
                let mut ws = self.ws.write().unwrap_or_else(|e| e.into_inner());
                for r in &eager_roots {
                    ws.mark_warmed(r.clone());
                }
                let n = ws.insert_batch(built);
                let syms: usize = ws.globals.values().map(|v| v.len()).sum();
                (n, syms)
            };
            let msg = format!("svlang: indexed {} files ({} symbols) in {} ms", n, syms, t0.elapsed().as_millis());
            eprintln!("[{}]", msg);
            self.log(&msg);
            let status = self.current_status();
            self.notify("svlang/status", status);
            return;
        }

        self.spawn_background_index(eager_roots, exclude, t0);
    }

    fn spawn_background_index(&mut self, roots: Vec<PathBuf>, exclude: Vec<String>, t0: Instant) {
        self.indexing.store(true, Ordering::SeqCst);
        self.notify("svlang/status", obj(vec![("state", "indexing".into())]));

        let ws_arc = self.ws.clone();
        let indexing_flag = self.indexing.clone();
        let cancel_flag = self.cancel_index.clone();
        let out_arc = self.out.clone();
        let status_handle = self.linter.as_ref().map(|l| l.status_handle());

        let _ = std::thread::Builder::new().name("svlang-index".into()).spawn(move || {
            let (all_paths, _) = Workspace::collect_paths_lockfree(&roots, &exclude, 50_000, None);
            let mut inserted_total = 0usize;
            for chunk in all_paths.chunks(250) {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                let missing: Vec<PathBuf> = {
                    let ws = ws_arc.read().unwrap_or_else(|e| e.into_inner());
                    chunk.iter().filter(|p| !ws.by_path.contains_key(*p)).cloned().collect()
                };
                if missing.is_empty() {
                    continue;
                }
                let built = Workspace::build_batch(&missing);
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                let mut ws = ws_arc.write().unwrap_or_else(|e| e.into_inner());
                inserted_total += ws.insert_batch(built);
            }
            {
                let mut ws = ws_arc.write().unwrap_or_else(|e| e.into_inner());
                for r in roots {
                    ws.mark_warmed(r);
                }
            }
            indexing_flag.store(false, Ordering::SeqCst);
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let (files, symbols, global_syms) = {
                let ws = ws_arc.read().unwrap_or_else(|e| e.into_inner());
                let files = ws.files.iter().flatten().count();
                let symbols: usize = ws.files.iter().flatten().map(|f| f.symbols.len()).sum();
                let global_syms: usize = ws.globals.values().map(|v| v.len()).sum();
                (files, symbols, global_syms)
            };
            let msg = format!(
                "svlang: indexed {} files ({} symbols) in {} ms",
                inserted_total,
                global_syms,
                t0.elapsed().as_millis()
            );
            eprintln!("[{}]", msg);
            crate::lint::send(
                &out_arc,
                &obj(vec![
                    ("jsonrpc", "2.0".into()),
                    ("method", "window/logMessage".into()),
                    ("params", obj(vec![("type", 4u32.into()), ("message", msg.into())])),
                ]),
            );
            let st = status_handle.map(|h| h.snapshot()).unwrap_or_default();
            crate::lint::send(
                &out_arc,
                &obj(vec![
                    ("jsonrpc", "2.0".into()),
                    ("method", "svlang/status".into()),
                    ("params", st.to_json(Some((files, symbols)))),
                ]),
            );
        });
    }

    /// Returns an unwarmed subproject root to index on demand for `path`, or `None` if
    /// `path` already sits inside an indexed workspace/subproject tree.
    fn unwarmed_subproject_for(&self, path: &Path) -> Option<PathBuf> {
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let enclosing_wr = self.roots.iter().find(|wr| path.starts_with(wr));

        let in_skipped_under = |base: &Path| -> bool {
            path.strip_prefix(base).ok().is_some_and(|rel| {
                rel.parent()
                    .into_iter()
                    .flat_map(|p| p.components())
                    .any(|c| c.as_os_str().to_str().is_some_and(crate::workspace::is_skipped_dir_name))
            })
        };

        if let Some(wr) = enclosing_wr {
            if ws.is_warmed(wr) && !in_skipped_under(wr) {
                return None;
            }
        }

        let cand = if let Some(proj) = ws.project_root_for_path(path) {
            let within_ws = enclosing_wr.is_none_or(|wr| proj.starts_with(wr));
            if within_ws && !in_skipped_under(&proj) {
                Some(proj)
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            let p1 = path.parent()?;
            if let Some(p2) = p1.parent() {
                if enclosing_wr.is_none_or(|wr| p2 != wr && p2.starts_with(wr)) {
                    return Some(p2.to_path_buf());
                }
            }
            if enclosing_wr.is_none_or(|wr| p1 != wr && p1.starts_with(wr)) {
                return Some(p1.to_path_buf());
            }
            None
        })?;

        (!ws.is_warmed(&cand)).then_some(cand)
    }

    /// Indexes the enclosing subproject of `path` when opened inside an unindexed
    /// multi-repo parent workspace.
    fn warm_subproject_async(&mut self, path: &Path) {
        let Some(sub_root) = self.unwarmed_subproject_for(path) else {
            return;
        };
        let exclude = self.exclude.clone();
        let (quick, truncated) = Workspace::collect_paths_lockfree(
            std::slice::from_ref(&sub_root),
            &exclude,
            200,
            Some(Instant::now() + Duration::from_millis(20)),
        );
        if !truncated {
            let missing: Vec<PathBuf> = {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                quick.into_iter().filter(|p| !ws.by_path.contains_key(p)).collect()
            };
            let built = Workspace::build_batch(&missing);
            {
                let mut ws = self.ws.write().unwrap_or_else(|e| e.into_inner());
                ws.mark_warmed(sub_root);
                ws.insert_batch(built);
            }
            let status = self.current_status();
            self.notify("svlang/status", status);
        } else {
            self.ws.write().unwrap_or_else(|e| e.into_inner()).mark_warmed(sub_root.clone());
            self.spawn_background_index(vec![sub_root], exclude, Instant::now());
        }
    }

    /// Ensures `path` is indexed, and warms up to 200 sibling/subproject files on demand
    /// when `path`'s enclosing subproject hasn't been indexed yet.
    fn ensure_file(&self, path: &Path) -> Option<usize> {
        let warm_root = self.unwarmed_subproject_for(path);
        if warm_root.is_none() {
            let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
            if let Some(f) = ws.file_id(path) {
                return Some(f);
            }
        }

        let mut to_insert: Vec<FileIndex> = Vec::new();
        let need_main = {
            let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
            ws.file_id(path).is_none()
        };
        if need_main {
            let t = std::fs::read_to_string(path).ok()?;
            to_insert.push(FileIndex::build(path, t));
        }

        if let Some(ref wroot) = warm_root {
            let (cand, _) = Workspace::collect_paths_lockfree(
                std::slice::from_ref(wroot),
                &self.exclude,
                200,
                Some(Instant::now() + Duration::from_millis(25)),
            );
            let missing: Vec<PathBuf> = {
                let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
                cand.into_iter().filter(|p| p != path && !ws.by_path.contains_key(p)).collect()
            };
            to_insert.extend(Workspace::build_batch(&missing));
        }

        let mut ws = self.ws.write().unwrap_or_else(|e| e.into_inner());
        if let Some(wroot) = warm_root {
            ws.mark_warmed(wroot);
        }
        ws.insert_batch(to_insert);
        ws.file_id(path)
    }

    /// The file and byte offset of a TextDocumentPositionParams.
    fn at(&self, p: &Json) -> Option<(usize, u32)> {
        let path = uri_to_path(p.at(&["textDocument", "uri"]).as_str()?);
        let f = self.ensure_file(&path)?;
        let line = p.at(&["position", "line"]).as_u32()?;
        let col = p.at(&["position", "character"]).as_u32()?;
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        Some((f, ws.file(f).offset(line, col)))
    }

    fn document_symbols(&self, p: &Json) -> Json {
        let Some(uri) = p.at(&["textDocument", "uri"]).as_str() else {
            return Json::Arr(vec![]);
        };
        let path = uri_to_path(uri);
        let Some(f) = self.ensure_file(&path) else {
            return Json::Arr(vec![]);
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let file = ws.file(f);
        let mut kids: HashMap<Option<u32>, Vec<u32>> = HashMap::new();
        for (i, s) in file.symbols.iter().enumerate() {
            if !s.local || s.kind == SymKind::EnumMember && s.parent.is_some_and(|p| !file.symbols[p as usize].local) {
                kids.entry(s.parent).or_default().push(i as u32);
            }
        }
        fn build(ws: &Workspace, f: usize, kids: &HashMap<Option<u32>, Vec<u32>>, parent: Option<u32>) -> Vec<Json> {
            let file = ws.file(f);
            let mut out = Vec::new();
            for &i in kids.get(&parent).map(|v| v.as_slice()).unwrap_or(&[]) {
                let s = &file.symbols[i as usize];
                let in_class = s.parent.is_some_and(|p| file.symbols[p as usize].kind == SymKind::Class);
                let mut detail = s.kind.label().to_string();
                if let Some(t) = &s.type_name {
                    if matches!(s.kind, SymKind::Variable | SymKind::Port | SymKind::Instance | SymKind::Parameter) {
                        detail = t.clone();
                    } else if s.kind == SymKind::Class {
                        detail = format!("extends {}", t);
                    }
                }
                let name = match &s.scope_class {
                    Some(c) => format!("{}::{}", c, s.name),
                    None => s.name.clone(),
                };
                out.push(obj(vec![
                    ("name", name.into()),
                    ("detail", detail.into()),
                    ("kind", symbol_kind(s, in_class).into()),
                    ("range", range_in(ws, f, s.start, s.end.max(s.sel_end))),
                    ("selectionRange", range_in(ws, f, s.sel_start, s.sel_end)),
                    ("children", Json::Arr(build(ws, f, kids, Some(i)))),
                ]));
            }
            out
        }
        Json::Arr(build(&ws, f, &kids, None))
    }

    fn workspace_symbols(&self, p: &Json) -> Json {
        let q = p.get("query").as_str().unwrap_or("").to_lowercase();
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let mut hits: Vec<(u32, SymRef)> = Vec::new();
        for (name, refs) in &ws.globals {
            let Some(score) = fuzzy(&q, name) else {
                continue;
            };
            for &r in refs {
                if ws.sym(r).kind == SymKind::EnumMember && q.is_empty() {
                    continue;
                }
                hits.push((score, r));
            }
        }
        hits.sort_by_key(|&(s, r)| (s, !ws.sym(r).kind.is_type(), ws.sym(r).name.len()));
        hits.truncate(500);
        Json::Arr(
            hits.into_iter()
                .map(|(_, r)| {
                    let s = ws.sym(r);
                    let file = ws.file(r.0);
                    let container =
                        s.parent.map(|p| file.symbols[p as usize].name.clone()).or_else(|| s.scope_class.clone());
                    let in_class = s.parent.is_some_and(|p| file.symbols[p as usize].kind == SymKind::Class)
                        || s.scope_class.is_some();
                    let mut kv = vec![
                        ("name", s.name.clone().into()),
                        ("kind", symbol_kind(s, in_class).into()),
                        ("location", location_in(&ws, r)),
                    ];
                    if let Some(c) = container {
                        kv.push(("containerName", c.into()));
                    }
                    obj(kv)
                })
                .collect(),
        )
    }

    fn definition(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let defs = ws.definitions(f, off);
        Json::Arr(defs.into_iter().take(50).map(|r| location_in(&ws, r)).collect())
    }

    fn type_definition(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let defs = ws.definitions(f, off);
        let mut out = Vec::new();
        for r in defs {
            let s = ws.sym(r);
            let t = match s.kind {
                SymKind::Class | SymKind::Typedef | SymKind::Module | SymKind::Interface => Some(s.name.clone()),
                _ => s.type_name.clone(),
            };
            if let Some(t) = t {
                for c in ws.scopes_named_near(
                    f,
                    &t,
                    &[SymKind::Class, SymKind::Module, SymKind::Interface, SymKind::Program, SymKind::Typedef],
                ) {
                    out.push(location_in(&ws, c));
                }
            }
            if !out.is_empty() {
                break;
            }
        }
        Json::Arr(out)
    }

    /// Every use of the name under the cursor, in all indexed files.
    /// Textual: names are matched, not resolved, so a common name like
    /// `data` finds every `data`.
    fn references(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let file = ws.file(f);
        let Some((s, e)) = word_at(&file.text, off) else {
            return Json::Arr(vec![]);
        };
        let word = file.text[s..e].to_string();
        let len = word.len() as u32;
        let mut out = Vec::new();
        for (fi, fx) in ws.files.iter().enumerate() {
            let Some(fx) = fx else { continue };
            if let Some(offs) = fx.idents.get(&word) {
                for &o in offs {
                    out.push(obj(vec![
                        ("uri", path_to_uri(&fx.path).into()),
                        ("range", range_in(&ws, fi, o, o + len)),
                    ]));
                    if out.len() >= 5000 {
                        return Json::Arr(out);
                    }
                }
            }
        }
        Json::Arr(out)
    }

    fn highlights(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let file = ws.file(f);
        let Some((s, e)) = word_at(&file.text, off) else {
            return Json::Arr(vec![]);
        };
        let word = &file.text[s..e];
        let len = word.len() as u32;
        let offs = file.idents.get(word).cloned().unwrap_or_default();
        Json::Arr(offs.into_iter().map(|o| obj(vec![("range", range_in(&ws, f, o, o + len))])).collect())
    }

    fn hover(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let defs = ws.definitions(f, off);
        let Some(&r) = defs.first() else {
            return Json::Null;
        };
        let s = ws.sym(r);
        let file = ws.file(r.0);
        let mut md = format!("```systemverilog\n{}\n```\n", s.detail);
        let container = s
            .parent
            .map(|p| &file.symbols[p as usize])
            .map(|c| format!("{} `{}`", c.kind.label(), c.name))
            .or_else(|| s.scope_class.as_ref().map(|c| format!("class `{}`", c)));
        let (line, _) = file.position(s.sel_start);
        let fname = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        md.push_str(&format!(
            "\n*{}*{} · {}:{}",
            s.kind.label(),
            container.map(|c| format!(" in {}", c)).unwrap_or_default(),
            fname,
            line + 1
        ));
        if !s.doc.is_empty() {
            md.push_str("\n\n---\n\n");
            md.push_str(&s.doc);
        }
        if defs.len() > 1 {
            md.push_str(&format!("\n\n*{} more definitions*", defs.len() - 1));
        }
        let file_here = ws.file(f);
        let word_span = word_at(&file_here.text, off).unwrap_or((off as usize, off as usize));
        obj(vec![
            ("contents", obj(vec![("kind", "markdown".into()), ("value", md.into())])),
            ("range", range_in(&ws, f, word_span.0 as u32, word_span.1 as u32)),
        ])
    }

    fn completion(&self, p: &Json) -> Json {
        let Some((f, off)) = self.at(p) else {
            return Json::Null;
        };
        let ws = self.ws.read().unwrap_or_else(|e| e.into_inner());
        let text = &ws.file(f).text;
        // The word typed so far, up to the cursor.
        let b = text.as_bytes();
        let mut s = (off as usize).min(b.len());
        while s > 0 && crate::text::is_ident_byte(b[s - 1]) && b[s - 1] != b'$' {
            s -= 1;
        }
        let prefix = text[s..off as usize].to_string();
        let before = if s > 0 { b[s - 1] } else { b'\n' };
        let mut items: Vec<Json> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut add = |items: &mut Vec<Json>, label: &str, kind: u32, detail: &str, doc: &str, sort: &str| {
            if !seen.insert(label.to_string()) {
                return;
            }
            let mut kv =
                vec![("label", label.into()), ("kind", kind.into()), ("sortText", format!("{}{}", sort, label).into())];
            if !detail.is_empty() {
                kv.push(("detail", detail.into()));
            }
            if !doc.is_empty() {
                kv.push(("documentation", obj(vec![("kind", "markdown".into()), ("value", doc.into())])));
            }
            items.push(obj(kv));
        };
        if before == b'`' {
            let mut macros: Vec<SymRef> =
                ws.globals.values().flatten().copied().filter(|&r| ws.sym(r).kind == SymKind::Macro).collect();
            macros.sort_by_key(|&r| ws.proximity(f, r.0));
            for r in macros {
                let sy = ws.sym(r);
                add(&mut items, &sy.name, 15, sy.detail.lines().next().unwrap_or(""), &sy.doc, "1");
            }
            for &(m, d) in UVM_MACROS {
                add(&mut items, m, 15, d, "", "2");
            }
            for d in DIRECTIVES {
                add(&mut items, d, 14, "directive", "", "3");
            }
            return obj(vec![("isIncomplete", false.into()), ("items", Json::Arr(items))]);
        }
        if before == b'$' {
            for &(t, d) in SYSTEM_TASKS {
                add(&mut items, t, 3, d, "", "1");
            }
            return obj(vec![("isIncomplete", false.into()), ("items", Json::Arr(items))]);
        }
        let sep = chain_sep(text, s);
        if sep != Sep::None {
            let chain = chain_before(text, s);
            if chain.last().is_some_and(|(_, n)| n == "type_id") {
                for (m, d) in [("create", "static function T create(string name, uvm_component parent = null)"), ("get", "static function type_id get()"), ("set_type_override", "static function void set_type_override(uvm_object_wrapper override_type, bit replace = 1)"), ("set_inst_override", "static function void set_inst_override(uvm_object_wrapper override_type, string inst_path, uvm_component parent = null)")] {
                    add(&mut items, m, 2, d, "", "1");
                }
                return obj(vec![("isIncomplete", false.into()), ("items", Json::Arr(items))]);
            }
            if let Some(scope) = ws.resolve_chain(f, off, &chain, sep) {
                let is_class = ws.sym(scope).kind == SymKind::Class;
                for (depth, r) in ws.members(scope).into_iter().enumerate() {
                    let sy = ws.sym(r);
                    if sy.kind == SymKind::Macro {
                        continue;
                    }
                    let own = r.0 == scope.0 && sy.parent == Some(scope.1);
                    add(
                        &mut items,
                        &sy.name,
                        completion_kind(sy, true),
                        &sy.detail,
                        &sy.doc,
                        if own { "1" } else { "2" },
                    );
                    let _ = depth;
                }
                if is_class && sep == Sep::Scope {
                    add(&mut items, "type_id", 7, "factory registration (`uvm_*_utils)", "", "0");
                }
            }
            return obj(vec![("isIncomplete", false.into()), ("items", Json::Arr(items))]);
        }
        // Plain names: scope first, then workspace types, then keywords.
        let low = prefix.to_lowercase();
        let matches = |n: &str| low.is_empty() || fuzzy(&low, n).is_some();
        let vis = ws.visible(f, off);
        for r in vis {
            let sy = ws.sym(r);
            if sy.kind == SymKind::Macro || !matches(&sy.name) {
                continue;
            }
            let in_class = sy.parent.is_some_and(|p| ws.file(r.0).symbols[p as usize].kind == SymKind::Class);
            add(&mut items, &sy.name, completion_kind(sy, in_class), &sy.detail, &sy.doc, "1");
        }
        let mut capped = false;
        if !low.is_empty() {
            let mut types: Vec<SymRef> = ws
                .globals
                .iter()
                .filter(|(n, _)| matches(n))
                .flat_map(|(_, v)| v.iter().copied())
                .filter(|&r| {
                    let sy = ws.sym(r);
                    sy.parent.is_none() && sy.kind != SymKind::Macro
                })
                .collect();
            types.sort_by_key(|&r| (!ws.sym(r).kind.is_type(), ws.proximity(f, r.0), ws.sym(r).name.len()));
            if types.len() > 400 {
                types.truncate(400);
                capped = true;
            }
            for r in types {
                let sy = ws.sym(r);
                add(&mut items, &sy.name, completion_kind(sy, false), &sy.detail, &sy.doc, "2");
            }
        }
        for k in KEYWORDS {
            if matches(k) {
                add(&mut items, k, 14, "", "", "3");
            }
        }
        obj(vec![("isIncomplete", capped.into()), ("items", Json::Arr(items))])
    }
}

fn range_in(ws: &Workspace, f: usize, a: u32, b: u32) -> Json {
    let file = ws.file(f);
    let (l0, c0) = file.position(a);
    let (l1, c1) = file.position(b);
    obj(vec![
        ("start", obj(vec![("line", l0.into()), ("character", c0.into())])),
        ("end", obj(vec![("line", l1.into()), ("character", c1.into())])),
    ])
}

fn location_in(ws: &Workspace, r: SymRef) -> Json {
    let s = ws.sym(r);
    obj(vec![("uri", path_to_uri(&ws.file(r.0).path).into()), ("range", range_in(ws, r.0, s.sel_start, s.sel_end))])
}

fn is_explicit_project_root(dir: &Path) -> bool {
    dir.join(".git").exists()
        || dir.join(".svlang-lint").exists()
        || dir.join("svlang_lint_config.py").exists()
        || dir.join(".svlang-lint.py").exists()
        || dir.join(".svlang-lint.json").exists()
        || dir.join("svlang-lint.json").exists()
}

fn discover_uvm_src() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("UVM_HOME") {
        let p = PathBuf::from(&h);
        let src = p.join("src");
        if src.is_dir() {
            return Some(src);
        }
        if p.join("uvm_pkg.sv").is_file() {
            return Some(p);
        }
    }
    let cache_base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let svlang_cache = cache_base.join("svlang");
    let rd = std::fs::read_dir(&svlang_cache).ok()?;
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("uvm-core")))
        .collect();
    dirs.sort();
    dirs.reverse();
    for d in dirs {
        let src = d.join("src");
        if src.join("uvm_pkg.sv").is_file() {
            return Some(src);
        }
    }
    None
}

/// Subsequence match of a lowercase query; lower scores are better.
pub fn fuzzy(q: &str, name: &str) -> Option<u32> {
    if q.is_empty() {
        return Some(0);
    }
    let n = name.to_lowercase();
    if n == q {
        return Some(0);
    }
    if n.starts_with(q) {
        return Some(1);
    }
    if n.contains(q) {
        return Some(2);
    }
    let mut it = n.chars();
    for c in q.chars() {
        it.by_ref().find(|&x| x == c)?;
    }
    Some(3)
}

fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let r = &rest[i + 1..];
        let (name, tail) = if let Some(r2) = r.strip_prefix('{') {
            match r2.find('}') {
                Some(j) => (&r2[..j], &r2[j + 1..]),
                None => (r2, ""),
            }
        } else {
            let n = r.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
            (&r[..n], &r[n..])
        };
        out.push_str(&std::env::var(name).unwrap_or_default());
        rest = tail;
    }
    out.push_str(rest);
    if let Some(r) = out.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME") {
            return format!("{}/{}", h.to_string_lossy(), r);
        }
    }
    out
}

fn str_list(j: &Json, key: &str) -> Option<Vec<String>> {
    let v = j.get(key);
    if v.is_null() {
        return None;
    }
    Some(v.as_arr().iter().filter_map(|x| x.as_str().map(|s| expand_env(s.trim()))).filter(|s| !s.is_empty()).collect())
}

fn apply_editor_json(ed: &mut svlang_lint::EditorOptions, j: &Json) {
    if !j.get("preset").is_null() {
        ed.preset = j.get("preset").as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    }
    if let Some(cp) = j.get("configPath").as_str() {
        let expanded = expand_env(cp.trim());
        ed.config_path = if expanded.is_empty() { None } else { Some(PathBuf::from(expanded)) };
    }
    if let Some(pref) = j.get("configPreference").as_str() {
        ed.config_preference = pref.trim().to_string();
    }
    if let Some(req) = j.get("requireConfig").as_bool() {
        ed.require_config = req;
    }
    if let Some(sd) = j.get("syntaxDiagnostics").as_bool() {
        ed.syntax_diagnostics = sd;
    }
    if let Some(sel) = str_list(j, "select") {
        ed.select = sel;
    }
    if let Some(ign) = str_list(j, "ignore") {
        ed.ignore = ign;
    }
    if let Some(fl) = str_list(j, "filelists") {
        ed.filelists = fl;
    }
    if let Some(defs) = str_list(j, "defines") {
        ed.defines = defs;
    }
    if let Some(incs) = str_list(j, "includeDirs") {
        ed.include_dirs = incs;
    }
}

pub(crate) fn symbol_kind(s: &Symbol, in_class: bool) -> u32 {
    match s.kind {
        SymKind::Module | SymKind::Program | SymKind::Checker => 2,
        SymKind::Interface => 11,
        SymKind::Package => 4,
        SymKind::Class => 5,
        SymKind::Function | SymKind::Task if s.name == "new" => 9,
        SymKind::Function | SymKind::Task if in_class => 6,
        SymKind::Function | SymKind::Task => 12,
        SymKind::Typedef if s.detail.contains("enum") => 10,
        SymKind::Typedef if s.detail.contains("struct") || s.detail.contains("union") => 23,
        SymKind::Typedef => 26,
        SymKind::EnumMember => 22,
        SymKind::Constraint => 7,
        SymKind::Covergroup => 19,
        SymKind::Property | SymKind::Sequence | SymKind::Clocking => 24,
        SymKind::Parameter | SymKind::Macro | SymKind::Genvar => 14,
        SymKind::Variable if in_class => 8,
        SymKind::Variable | SymKind::Port => 13,
        SymKind::Instance => 19,
        SymKind::Modport => 11,
    }
}

fn completion_kind(s: &Symbol, in_class: bool) -> u32 {
    match s.kind {
        SymKind::Module | SymKind::Program | SymKind::Checker => 9,
        SymKind::Interface | SymKind::Modport => 8,
        SymKind::Package => 9,
        SymKind::Class => 7,
        SymKind::Function | SymKind::Task if s.name == "new" => 4,
        SymKind::Function | SymKind::Task if in_class => 2,
        SymKind::Function | SymKind::Task => 3,
        SymKind::Typedef if s.detail.contains("enum") => 13,
        SymKind::Typedef => 22,
        SymKind::EnumMember => 20,
        SymKind::Constraint | SymKind::Covergroup => 10,
        SymKind::Property | SymKind::Sequence | SymKind::Clocking => 23,
        SymKind::Parameter | SymKind::Genvar => 21,
        SymKind::Macro => 15,
        SymKind::Variable if in_class => 5,
        SymKind::Variable | SymKind::Port | SymKind::Instance => 6,
    }
}

const KEYWORDS: &[&str] = &[
    "module",
    "endmodule",
    "interface",
    "endinterface",
    "program",
    "endprogram",
    "package",
    "endpackage",
    "class",
    "endclass",
    "function",
    "endfunction",
    "task",
    "endtask",
    "begin",
    "end",
    "fork",
    "join",
    "join_any",
    "join_none",
    "if",
    "else",
    "case",
    "endcase",
    "default",
    "for",
    "foreach",
    "forever",
    "repeat",
    "while",
    "do",
    "return",
    "break",
    "continue",
    "always",
    "always_comb",
    "always_ff",
    "always_latch",
    "initial",
    "final",
    "assign",
    "posedge",
    "negedge",
    "logic",
    "bit",
    "byte",
    "int",
    "integer",
    "longint",
    "shortint",
    "real",
    "string",
    "time",
    "void",
    "event",
    "wire",
    "reg",
    "input",
    "output",
    "inout",
    "ref",
    "const",
    "static",
    "automatic",
    "virtual",
    "pure",
    "extern",
    "local",
    "protected",
    "rand",
    "randc",
    "constraint",
    "solve",
    "before",
    "dist",
    "inside",
    "with",
    "soft",
    "unique",
    "priority",
    "typedef",
    "enum",
    "struct",
    "union",
    "packed",
    "parameter",
    "localparam",
    "import",
    "export",
    "extends",
    "implements",
    "new",
    "this",
    "super",
    "null",
    "covergroup",
    "endgroup",
    "coverpoint",
    "cross",
    "bins",
    "illegal_bins",
    "ignore_bins",
    "property",
    "endproperty",
    "sequence",
    "endsequence",
    "assert",
    "assume",
    "cover",
    "disable",
    "iff",
    "wait",
    "clocking",
    "endclocking",
    "modport",
    "generate",
    "endgenerate",
    "genvar",
    "signed",
    "unsigned",
    "type",
];

const DIRECTIVES: &[&str] = &[
    "include",
    "define",
    "ifdef",
    "ifndef",
    "elsif",
    "else",
    "endif",
    "undef",
    "timescale",
    "default_nettype",
    "resetall",
    "pragma",
    "line",
    "celldefine",
    "endcelldefine",
];

const UVM_MACROS: &[(&str, &str)] = &[
    ("uvm_info", "`uvm_info(ID, MSG, VERBOSITY)"),
    ("uvm_warning", "`uvm_warning(ID, MSG)"),
    ("uvm_error", "`uvm_error(ID, MSG)"),
    ("uvm_fatal", "`uvm_fatal(ID, MSG)"),
    ("uvm_component_utils", "`uvm_component_utils(T)"),
    ("uvm_component_param_utils", "`uvm_component_param_utils(T #(P))"),
    ("uvm_component_utils_begin", "`uvm_component_utils_begin(T)"),
    ("uvm_component_utils_end", "`uvm_component_utils_end"),
    ("uvm_object_utils", "`uvm_object_utils(T)"),
    ("uvm_object_param_utils", "`uvm_object_param_utils(T #(P))"),
    ("uvm_object_utils_begin", "`uvm_object_utils_begin(T)"),
    ("uvm_object_utils_end", "`uvm_object_utils_end"),
    ("uvm_field_int", "`uvm_field_int(ARG, FLAG)"),
    ("uvm_field_object", "`uvm_field_object(ARG, FLAG)"),
    ("uvm_field_string", "`uvm_field_string(ARG, FLAG)"),
    ("uvm_field_enum", "`uvm_field_enum(T, ARG, FLAG)"),
    ("uvm_field_array_int", "`uvm_field_array_int(ARG, FLAG)"),
    ("uvm_field_queue_int", "`uvm_field_queue_int(ARG, FLAG)"),
    ("uvm_do", "`uvm_do(SEQ_OR_ITEM)"),
    ("uvm_do_with", "`uvm_do_with(SEQ_OR_ITEM, CONSTRAINTS)"),
    ("uvm_do_on", "`uvm_do_on(SEQ_OR_ITEM, SEQR)"),
    ("uvm_create", "`uvm_create(SEQ_OR_ITEM)"),
    ("uvm_send", "`uvm_send(SEQ_OR_ITEM)"),
    ("uvm_rand_send", "`uvm_rand_send(SEQ_OR_ITEM)"),
    ("uvm_declare_p_sequencer", "`uvm_declare_p_sequencer(SEQUENCER)"),
    ("uvm_analysis_imp_decl", "`uvm_analysis_imp_decl(SFX)"),
    ("uvm_register_cb", "`uvm_register_cb(T, CB)"),
    ("uvm_do_callbacks", "`uvm_do_callbacks(T, CB, METHOD)"),
];

const SYSTEM_TASKS: &[(&str, &str)] = &[
    ("display", "$display(fmt, ...)"),
    ("write", "$write(fmt, ...)"),
    ("sformatf", "$sformatf(fmt, ...) -> string"),
    ("sformat", "$sformat(str, fmt, ...)"),
    ("time", "$time -> time"),
    ("realtime", "$realtime -> realtime"),
    ("finish", "$finish"),
    ("stop", "$stop"),
    ("random", "$random"),
    ("urandom", "$urandom -> int unsigned"),
    ("urandom_range", "$urandom_range(max, min = 0)"),
    ("cast", "$cast(dest, src) -> int"),
    ("bits", "$bits(expr)"),
    ("clog2", "$clog2(n)"),
    ("size", "$size(array)"),
    ("isunknown", "$isunknown(expr)"),
    ("onehot", "$onehot(expr)"),
    ("onehot0", "$onehot0(expr)"),
    ("countones", "$countones(expr)"),
    ("past", "$past(expr, n = 1)"),
    ("rose", "$rose(expr)"),
    ("fell", "$fell(expr)"),
    ("stable", "$stable(expr)"),
    ("changed", "$changed(expr)"),
    ("fopen", "$fopen(name, mode)"),
    ("fclose", "$fclose(fd)"),
    ("fdisplay", "$fdisplay(fd, fmt, ...)"),
    ("fwrite", "$fwrite(fd, fmt, ...)"),
    ("readmemh", "$readmemh(file, mem)"),
    ("readmemb", "$readmemb(file, mem)"),
    ("value$plusargs", "$value$plusargs(fmt, var)"),
    ("test$plusargs", "$test$plusargs(name)"),
    ("typename", "$typename(expr)"),
    ("error", "$error(fmt, ...)"),
    ("warning", "$warning(fmt, ...)"),
    ("info", "$info(fmt, ...)"),
    ("fatal", "$fatal(level, fmt, ...)"),
    ("dumpfile", "$dumpfile(name)"),
    ("dumpvars", "$dumpvars(levels, scope)"),
    ("signed", "$signed(expr)"),
    ("unsigned", "$unsigned(expr)"),
];

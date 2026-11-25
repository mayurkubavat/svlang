//! Diagnostics and quick fixes from the linter (`svlang-lint`), for the files open
//! in the editor.
//!
//! Linting runs on a thread of its own, so navigation requests are answered
//! while a check is under way. Every edit asks for a check; the thread waits
//! for a pause in typing, then checks the project once with the editor's
//! text standing in for the open files (`svlang::syntax::overlay`) and publishes
//! each open file's diagnostics. What it published is kept, so a code action
//! request is answered from it without linting again.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::json::{obj, Json};
use crate::text::{path_to_uri, word_at, LineIndex};

/// Where the server writes messages: shared with the lint thread.
pub type Out = Arc<Mutex<Box<dyn Write + Send>>>;

/// Writes one framed message.
pub fn send(out: &Out, msg: &Json) {
    let body = msg.to_string();
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

/// How long typing must pause before a check starts.
const QUIET: Duration = Duration::from_millis(250);

/// A text edit of a quick fix, in LSP positions.
#[derive(Clone)]
struct Edit {
    title: String,
    range: [u32; 4],
    text: String,
    preferred: bool,
}

/// One published diagnostic.
#[derive(Clone)]
struct Diagnostic {
    range: [u32; 4],
    json: Json,
    fixes: Vec<Edit>,
}

/// When automatic lint checks run in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintTrigger {
    OnType,
    OnSave,
    Off,
}

impl LintTrigger {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "onSave" | "on_save" | "save" => LintTrigger::OnSave,
            "off" | "manual" | "none" | "false" => LintTrigger::Off,
            _ => LintTrigger::OnType,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LintTrigger::OnType => "onType",
            LintTrigger::OnSave => "onSave",
            LintTrigger::Off => "off",
        }
    }
}

#[derive(Clone, Default)]
pub struct LintStatus {
    pub state: String,
    pub has_config: bool,
    pub config_path: Option<String>,
    pub preset: String,
    pub trigger: String,
    pub errors: u32,
    pub warnings: u32,
}

impl LintStatus {
    pub fn to_json(&self, counts: Option<(usize, usize)>) -> Json {
        let mut fields = vec![
            ("state", (if self.state.is_empty() { "ready" } else { &self.state }).into()),
            ("hasConfig", self.has_config.into()),
            ("preset", (if self.preset.is_empty() { "default" } else { &self.preset }).into()),
            ("trigger", (if self.trigger.is_empty() { "onType" } else { &self.trigger }).into()),
            ("errors", self.errors.into()),
            ("warnings", self.warnings.into()),
        ];
        if let Some((files, symbols)) = counts {
            fields.push(("files", (files as u32).into()));
            fields.push(("symbols", (symbols as u32).into()));
        }
        if let Some(cfg) = &self.config_path {
            fields.push(("configPath", cfg.as_str().into()));
        }
        obj(fields)
    }
}

#[derive(Clone)]
pub struct StatusHandle(Arc<Mutex<State>>);

impl StatusHandle {
    pub fn snapshot(&self) -> LintStatus {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).status.clone()
    }
}

#[derive(Default)]
struct State {
    /// Open files, with the URI the editor names each by.
    open: HashMap<PathBuf, String>,
    /// What was last published per open file.
    published: HashMap<PathBuf, Vec<Diagnostic>>,
    trigger: Option<LintTrigger>,
    editor: svlang_lint::EditorOptions,
    status: LintStatus,
}

enum WakeMsg {
    Debounced,
    AutoImmediate,
    ManualImmediate,
    Reconfigure,
}

/// The lint side of the server.
pub struct Linter {
    wake: Sender<WakeMsg>,
    state: Arc<Mutex<State>>,
    out: Out,
}

fn range_json(r: [u32; 4]) -> Json {
    let at = |l: u32, c: u32| obj(vec![("line", l.into()), ("character", c.into())]);
    obj(vec![("start", at(r[0], r[1])), ("end", at(r[2], r[3]))])
}

/// The file of a finding as the editor knows it.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The text of files, with their line indexes, read once per check.
#[derive(Default)]
struct Texts(HashMap<String, Option<(String, LineIndex)>>);

impl Texts {
    fn get(&mut self, path: &str) -> Option<&(String, LineIndex)> {
        self.0
            .entry(path.to_string())
            .or_insert_with(|| {
                let text = svlang::syntax::overlay::read(Path::new(path)).ok()?;
                let index = LineIndex::new(&text);
                Some((text, index))
            })
            .as_ref()
    }

    /// The leading blanks of line `line` (0-based) of `path`.
    fn indent(&mut self, path: &str, line: u32) -> String {
        let Some((text, _)) = self.get(path) else { return String::new() };
        let l = text.lines().nth(line as usize).unwrap_or("");
        l[..l.len() - l.trim_start().len()].to_string()
    }

    /// The LSP position of byte `offset` of `path`.
    fn position(&mut self, path: &str, offset: u32) -> Option<(u32, u32)> {
        let (text, index) = self.get(path)?;
        Some(index.position(text, offset))
    }

    /// The range of a finding at `line`:`col` (1-based, bytes): the word
    /// there, else one character; the line's code for column 0 (a finding
    /// on a macro call).
    fn range(&mut self, path: &str, line: u32, col: u32) -> [u32; 4] {
        let l = line.saturating_sub(1);
        let Some((text, index)) = self.get(path) else { return [l, 0, l, 1] };
        let start = index.offset(text, l, 0);
        if col == 0 {
            let rest = &text[start as usize..];
            let rest = &rest[..rest.find('\n').unwrap_or(rest.len())];
            let lead = (rest.len() - rest.trim_start().len()) as u32;
            let (_, c0) = index.position(text, start + lead);
            let (_, c1) = index.position(text, start + rest.trim_end().len() as u32);
            return [l, c0, l, c1.max(c0 + 1)];
        }
        let at = (start + col - 1).min(text.len() as u32);
        let end = match word_at(text, at) {
            Some((a, b)) if a as u32 == at => b as u32,
            _ => at + 1,
        };
        let (l0, c0) = index.position(text, at);
        let (l1, c1) = index.position(text, end);
        if l1 != l0 || c1 <= c0 {
            [l0, c0, l0, c0 + 1]
        } else {
            [l0, c0, l1, c1]
        }
    }
}

/// Files with their modification times: what a loaded project depends on.
type Stamps = Vec<(PathBuf, Option<SystemTime>)>;

/// The project of `root`, reloaded when its config, waivers, or editor options change.
struct Project {
    root: PathBuf,
    editor: svlang_lint::EditorOptions,
    loaded: Option<(Stamps, Result<svlang_lint::Workspace, String>)>,
}

impl Project {
    fn stamps(root: &Path, editor: &svlang_lint::EditorOptions, ws: Option<&svlang_lint::Workspace>) -> Stamps {
        let mut files: Vec<PathBuf> = Vec::new();
        let base = ws.map_or(root, |w| w.root()).to_path_buf();
        files.extend(ws.and_then(|w| w.config_file()).map(Path::to_path_buf));
        files.extend(editor.config_path.clone());
        for fl in &editor.filelists {
            let p = Path::new(fl);
            files.push(if p.is_relative() { base.join(p) } else { p.to_path_buf() });
        }
        for n in [
            ".svlang-lint/config.py",
            ".svlang-lint/config.json",
            "svlang_lint_config.py",
            ".svlang-lint.py",
            ".svlang-lint.json",
            "svlang-lint.json",
            ".svlang-lint/waivers.json",
            ".svlang-lint/baseline.json",
        ] {
            files.push(base.join(n));
        }
        files.sort();
        files.dedup();
        files.into_iter().map(|f| (f.clone(), std::fs::metadata(&f).and_then(|m| m.modified()).ok())).collect()
    }

    fn workspace(&mut self) -> &Result<svlang_lint::Workspace, String> {
        let stale = match &self.loaded {
            Some((stamps, ws)) => *stamps != Self::stamps(&self.root, &self.editor, ws.as_ref().ok()),
            None => true,
        };
        if stale {
            let ws = svlang_lint::Workspace::open_with_editor(&self.root, &self.editor);
            self.loaded = Some((Self::stamps(&self.root, &self.editor, ws.as_ref().ok()), ws));
        }
        &self.loaded.as_ref().expect("just loaded").1
    }
}

fn emit_status(out: &Out, status: &LintStatus) {
    send(
        out,
        &obj(vec![("jsonrpc", "2.0".into()), ("method", "svlang/status".into()), ("params", status.to_json(None))]),
    );
}

/// Checks the project and publishes each open file's diagnostics.
fn check(project: &mut Project, state: &Mutex<State>, out: &Out, manual: bool) {
    let (open, cur_trig, trig_str, preset_str, require_cfg, syntax_diags, has_explicit_cfg) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        let trig = st.trigger.unwrap_or(LintTrigger::OnType);
        let pstr = project.editor.preset.clone().unwrap_or_else(|| "default".into());
        (
            st.open.clone(),
            trig,
            trig.as_str().to_string(),
            pstr,
            project.editor.require_config,
            project.editor.syntax_diagnostics,
            project.editor.config_path.as_ref().is_some_and(|p| p.is_file()),
        )
    };
    let idle_state = |trig: LintTrigger, syn: bool| -> String {
        if trig != LintTrigger::Off {
            "ready".into()
        } else if syn {
            "syntax".into()
        } else {
            "off".into()
        }
    };
    if open.is_empty() {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        let trig = st.trigger.unwrap_or(LintTrigger::OnType);
        st.status.state = idle_state(trig, project.editor.syntax_diagnostics);
        st.status.errors = 0;
        st.status.warnings = 0;
        emit_status(out, &st.status);
        return;
    }

    // When automatic linting is `Off` (Mode 1) and not invoked via `Lint Now`:
    // if `syntax_diagnostics` is enabled, run fast compiler-only (`CMP-ERR-001`) checks; otherwise do nothing.
    let mut compiler_only = !manual && cur_trig == LintTrigger::Off;
    if compiler_only && !syntax_diags {
        return;
    }

    // Check `requireConfig` before running lint rules unless invoked manually (`Lint Now`).
    if !compiler_only && require_cfg && !manual {
        let has_any_cfg = match project.workspace() {
            Ok(ws) => ws.has_repo_config() || has_explicit_cfg,
            Err(_) => false,
        };
        if !has_any_cfg {
            if syntax_diags {
                compiler_only = true;
            } else {
                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                st.published.clear();
                let uris: Vec<String> = st.open.values().cloned().collect();
                st.status = LintStatus {
                    state: "standalone".into(),
                    has_config: false,
                    config_path: None,
                    preset: preset_str,
                    trigger: trig_str,
                    errors: 0,
                    warnings: 0,
                };
                emit_status(out, &st.status);
                drop(st);
                for u in uris {
                    publish(out, &u, &[]);
                }
                return;
            }
        }
    }

    if !compiler_only {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.status.state = "linting".into();
        st.status.trigger = trig_str.clone();
        st.status.preset = preset_str.clone();
        emit_status(out, &st.status);
    }

    let paths: Vec<PathBuf> = open.keys().cloned().collect();
    let (has_cfg, cfg_path, checked) = match project.workspace() {
        Ok(ws) => {
            let cfg_p = ws.config_file().map(|p| p.to_string_lossy().into_owned());
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if compiler_only {
                    ws.check_compiler_only(&paths)
                } else {
                    ws.check(&paths)
                }
            }))
            .unwrap_or_else(|_| Err("the linter stopped on an internal error".to_string()));
            (cfg_p.is_some(), cfg_p, res)
        }
        Err(e) => (false, None, Err(e.clone())),
    };
    let checked = match checked {
        Ok(c) => c,
        Err(e) => {
            let params = obj(vec![("type", 2u32.into()), ("message", format!("svlang-lint: {e}").into())]);
            send(
                out,
                &obj(vec![("jsonrpc", "2.0".into()), ("method", "window/logMessage".into()), ("params", params)]),
            );
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.status.state = "error".into();
            st.status.has_config = has_cfg;
            st.status.config_path = cfg_path;
            emit_status(out, &st.status);
            return;
        }
    };
    let by_canonical: HashMap<PathBuf, &PathBuf> = open.keys().map(|p| (canonical(p), p)).collect();
    let mut texts = Texts::default();
    let mut found: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    let mut total_errors = 0u32;
    let mut total_warnings = 0u32;
    for f in &checked.findings {
        let (file, line, col) = f.position_in(&checked.files);
        let Some(&path) = by_canonical.get(&canonical(Path::new(file))) else { continue };
        match f.severity {
            svlang_lint::Severity::Error => total_errors += 1,
            svlang_lint::Severity::Info => {}
            _ => total_warnings += 1,
        }
        let range = texts.range(file, line, col);
        let related: Vec<Json> = f
            .related
            .iter()
            .filter_map(|(loc, message)| {
                let file = checked.files.get(loc.file as usize).filter(|f| !f.is_empty() && loc.line > 0)?;
                let r = texts.range(file, loc.line, loc.column);
                let location =
                    obj(vec![("uri", path_to_uri(&canonical(Path::new(file))).into()), ("range", range_json(r))]);
                Some(obj(vec![("location", location), ("message", message.as_str().into())]))
            })
            .collect();
        let severity: u32 = match f.severity {
            svlang_lint::Severity::Error => 1,
            svlang_lint::Severity::Info => 3,
            _ => 2,
        };
        let diag_source = if f.rule == "CMP-ERR-001" { "svlang" } else { "svlang-lint" };
        let mut fields = vec![
            ("range", range_json(range)),
            ("severity", severity.into()),
            ("code", f.rule.into()),
            ("source", diag_source.into()),
            ("message", format!("{} ({})", f.message, f.name).into()),
        ];
        if !related.is_empty() {
            fields.push(("relatedInformation", Json::Arr(related)));
        }
        let fixes = f
            .fixes
            .iter()
            .filter(|x| x.safety != svlang_lint::fix::FixSafety::DisplayOnly)
            .filter(|x| canonical(Path::new(&x.path)) == canonical(path))
            .filter_map(|x| {
                let (l0, c0) = texts.position(&x.path, x.start)?;
                let (l1, c1) = texts.position(&x.path, x.end)?;
                Some(Edit {
                    title: if x.description.is_empty() { format!("Fix {}", f.rule) } else { x.description.clone() },
                    range: [l0, c0, l1, c1],
                    text: x.text.clone(),
                    preferred: x.safety == svlang_lint::fix::FixSafety::Safe,
                })
            })
            .collect();
        let mut fixes: Vec<Edit> = fixes;
        // Any finding but a compile error or a broken waiver can be waived
        // on its line: a comment on the line above, indented like it.
        if !matches!(f.rule, "CMP-ERR-001" | "SVL-WVR-002") {
            let indent = texts.indent(file, range[0]);
            fixes.push(Edit {
                title: format!("Waive {} on this line", f.rule),
                range: [range[0], 0, range[0], 0],
                text: format!("{indent}// svlang-lint-disable-next-line {}\n", f.rule),
                preferred: false,
            });
        }
        found.entry(path.clone()).or_default().push(Diagnostic { range, json: obj(fields), fixes });
    }
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    for (path, uri) in &open {
        // Closed while the check ran: its diagnostics were cleared already.
        if !state.open.contains_key(path) {
            continue;
        }
        let mut list = found.remove(path).unwrap_or_default();
        list.sort_by_key(|d| (d.range[0], d.range[1]));
        publish(out, uri, &list);
        state.published.insert(path.clone(), list);
    }
    let cur_trig = state.trigger.unwrap_or(LintTrigger::OnType);
    let fin_state = if manual {
        "ready"
    } else if compiler_only {
        if cur_trig == LintTrigger::Off {
            "syntax"
        } else {
            "standalone"
        }
    } else if cur_trig != LintTrigger::Off {
        "ready"
    } else {
        "off"
    };
    state.status = LintStatus {
        state: fin_state.into(),
        has_config: has_cfg,
        config_path: cfg_path,
        preset: preset_str,
        trigger: trig_str,
        errors: total_errors,
        warnings: total_warnings,
    };
    emit_status(out, &state.status);
}

fn publish(out: &Out, uri: &str, list: &[Diagnostic]) {
    let params =
        obj(vec![("uri", uri.into()), ("diagnostics", Json::Arr(list.iter().map(|d| d.json.clone()).collect()))]);
    send(
        out,
        &obj(vec![("jsonrpc", "2.0".into()), ("method", "textDocument/publishDiagnostics".into()), ("params", params)]),
    );
}

impl Linter {
    /// Starts the lint thread with initial `trigger` and `editor` options.
    pub fn start_with_options(
        root: PathBuf,
        initial_trigger: LintTrigger,
        editor: svlang_lint::EditorOptions,
        out: Out,
    ) -> Linter {
        let (wake, asked) = channel::<WakeMsg>();
        let has_cfg_initial = svlang_lint::project::find(None, None).is_some()
            || editor.config_path.as_ref().is_some_and(|p| p.is_file());
        let initial_state = State {
            open: HashMap::new(),
            published: HashMap::new(),
            trigger: Some(initial_trigger),
            editor: editor.clone(),
            status: LintStatus {
                state: if initial_trigger == LintTrigger::Off {
                    if editor.syntax_diagnostics {
                        "syntax".into()
                    } else {
                        "off".into()
                    }
                } else if editor.require_config && !has_cfg_initial {
                    "standalone".into()
                } else {
                    "ready".into()
                },
                has_config: has_cfg_initial,
                config_path: None,
                preset: editor.preset.clone().unwrap_or_else(|| "default".into()),
                trigger: initial_trigger.as_str().into(),
                errors: 0,
                warnings: 0,
            },
        };
        let state: Arc<Mutex<State>> = Arc::new(Mutex::new(initial_state));
        let (thread_state, thread_out) = (state.clone(), out.clone());
        let spawned = std::thread::Builder::new().name("lint".into()).stack_size(256 << 20).spawn(move || {
            let mut project = Project { root, editor, loaded: None };
            while let Ok(mut first) = asked.recv() {
                if matches!(first, WakeMsg::Debounced) {
                    loop {
                        match asked.recv_timeout(QUIET) {
                            Ok(WakeMsg::ManualImmediate) => {
                                first = WakeMsg::ManualImmediate;
                                break;
                            }
                            Ok(WakeMsg::AutoImmediate) => {
                                first = WakeMsg::AutoImmediate;
                                break;
                            }
                            Ok(WakeMsg::Reconfigure) => {
                                first = WakeMsg::Reconfigure;
                                break;
                            }
                            Ok(WakeMsg::Debounced) => continue,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
                if matches!(first, WakeMsg::Reconfigure) {
                    let (new_trig, new_editor, uris_to_clear) = {
                        let mut st = thread_state.lock().unwrap_or_else(|e| e.into_inner());
                        let trig = st.trigger.unwrap_or(LintTrigger::OnType);
                        let ed = st.editor.clone();
                        let fully_off = trig == LintTrigger::Off && !ed.syntax_diagnostics;
                        let uris: Vec<String> = if fully_off {
                            st.published.clear();
                            st.open.values().cloned().collect()
                        } else {
                            Vec::new()
                        };
                        st.status.trigger = trig.as_str().into();
                        st.status.preset = ed.preset.clone().unwrap_or_else(|| "default".into());
                        if fully_off {
                            st.status.state = "off".into();
                            st.status.errors = 0;
                            st.status.warnings = 0;
                        } else if trig == LintTrigger::Off {
                            st.status.state = "syntax".into();
                        }
                        (trig, ed, uris)
                    };
                    let syntax_active = new_editor.syntax_diagnostics;
                    if project.editor != new_editor {
                        project.editor = new_editor;
                        project.loaded = None;
                    }
                    if new_trig == LintTrigger::Off && !syntax_active {
                        for u in uris_to_clear {
                            publish(&thread_out, &u, &[]);
                        }
                        let st = thread_state.lock().unwrap_or_else(|e| e.into_inner());
                        emit_status(&thread_out, &st.status);
                        continue;
                    }
                }
                let manual = matches!(first, WakeMsg::ManualImmediate);
                check(&mut project, &thread_state, &thread_out, manual);
            }
        });
        if let Err(e) = spawned {
            eprintln!("[svlang-lsp] no lint thread: {e}");
        }
        Linter { wake, state, out }
    }

    fn auto_active(&self) -> bool {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.trigger.unwrap_or(LintTrigger::OnType) != LintTrigger::Off || st.editor.syntax_diagnostics
    }

    /// Asks for a debounced check (once typing pauses), if automatic diagnostics are active.
    pub fn request(&self) {
        if self.auto_active() {
            let _ = self.wake.send(WakeMsg::Debounced);
        }
    }

    /// Runs an immediate manual check regardless of `trigger` or `requireConfig`.
    pub fn lint_now(&self) {
        let _ = self.wake.send(WakeMsg::ManualImmediate);
    }

    /// Updates `trigger` and/or `editor` options dynamically without restarting the server.
    pub fn set_options(&self, trigger: Option<LintTrigger>, editor: Option<svlang_lint::EditorOptions>) {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = trigger {
                st.trigger = Some(t);
            }
            if let Some(ed) = editor {
                st.editor = ed;
            }
        }
        let _ = self.wake.send(WakeMsg::Reconfigure);
    }

    pub fn editor_options(&self) -> svlang_lint::EditorOptions {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).editor.clone()
    }

    pub fn status(&self) -> LintStatus {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).status.clone()
    }

    pub fn status_handle(&self) -> StatusHandle {
        StatusHandle(self.state.clone())
    }

    /// The editor opened `path`.
    pub fn opened(&self, path: &Path, uri: &str, text: &str) {
        svlang::syntax::overlay::set(path, text);
        let wake = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.open.insert(path.to_path_buf(), uri.to_string());
            st.trigger.unwrap_or(LintTrigger::OnType) != LintTrigger::Off || st.editor.syntax_diagnostics
        };
        if wake {
            let _ = self.wake.send(WakeMsg::Debounced);
        }
    }

    /// The editor changed `path`'s text in memory.
    pub fn changed(&self, path: &Path, uri: &str, text: &str) {
        svlang::syntax::overlay::set(path, text);
        let wake = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.open.insert(path.to_path_buf(), uri.to_string());
            let trig = st.trigger.unwrap_or(LintTrigger::OnType);
            trig == LintTrigger::OnType || (trig == LintTrigger::Off && st.editor.syntax_diagnostics)
        };
        if wake {
            let _ = self.wake.send(WakeMsg::Debounced);
        }
    }

    /// The editor saved `path`.
    pub fn saved(&self) {
        if self.auto_active() {
            let _ = self.wake.send(WakeMsg::AutoImmediate);
        }
    }

    /// The editor closed `path`: the file on disk counts again, and its
    /// diagnostics go away.
    pub fn closed(&self, path: &Path, uri: &str) {
        svlang::syntax::overlay::clear(path);
        let wake = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.open.remove(path);
            state.published.remove(path);
            state.trigger.unwrap_or(LintTrigger::OnType) != LintTrigger::Off || state.editor.syntax_diagnostics
        };
        publish(&self.out, uri, &[]);
        if wake {
            let _ = self.wake.send(WakeMsg::Debounced);
        }
    }

    /// Quick fixes for the diagnostics of `path` that touch lines `first..=last`,
    /// plus `source.fixAll.svlang` across all safe non-overlapping fixes in `path`.
    pub fn code_actions(&self, path: &Path, uri: &str, first: u32, last: u32, only: &[String]) -> Json {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let all_diags = state.published.get(path).map_or(&[][..], |l| &l[..]);
        let mut actions = Vec::new();
        let want_quickfix = only.is_empty() || only.iter().any(|k| k == "quickfix" || k.starts_with("quickfix."));
        let want_fix_all = only.is_empty() || only.iter().any(|k| k == "source.fixAll" || k == "source.fixAll.svlang");

        if want_quickfix {
            for d in all_diags {
                if d.range[2] < first || d.range[0] > last {
                    continue;
                }
                for fix in &d.fixes {
                    let edit = obj(vec![("range", range_json(fix.range)), ("newText", fix.text.as_str().into())]);
                    let changes = obj(vec![(uri, Json::Arr(vec![edit]))]);
                    actions.push(obj(vec![
                        ("title", fix.title.as_str().into()),
                        ("kind", "quickfix".into()),
                        ("diagnostics", Json::Arr(vec![d.json.clone()])),
                        ("isPreferred", fix.preferred.into()),
                        ("edit", obj(vec![("changes", changes)])),
                    ]));
                }
            }
        }

        if want_fix_all {
            // Collect safe (`preferred == true`, excluding inline waiver insertions) non-overlapping fixes across the file.
            let mut safe: Vec<&Edit> = all_diags
                .iter()
                .flat_map(|d| d.fixes.iter())
                .filter(|f| f.preferred && !f.title.starts_with("Waive "))
                .collect();
            safe.sort_by_key(|f| (f.range[0], f.range[1], f.range[2], f.range[3]));
            let mut chosen: Vec<Json> = Vec::new();
            let mut last_end: Option<(u32, u32)> = None;
            for f in safe {
                if let Some(le) = last_end {
                    if (f.range[0], f.range[1]) < le {
                        continue;
                    }
                }
                chosen.push(obj(vec![("range", range_json(f.range)), ("newText", f.text.as_str().into())]));
                last_end = Some((f.range[2], f.range[3]));
            }
            if !chosen.is_empty() {
                let n = chosen.len();
                let changes = obj(vec![(uri, Json::Arr(chosen))]);
                actions.push(obj(vec![
                    ("title", format!("Fix all {n} safe svlang-lint issue(s)").into()),
                    ("kind", "source.fixAll.svlang".into()),
                    ("isPreferred", true.into()),
                    ("edit", obj(vec![("changes", changes)])),
                ]));
            }
        }

        Json::Arr(actions)
    }
}

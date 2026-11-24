//! A project as an editor or a host program sees it: its configuration
//! loaded once, and checked again whenever the sources change.
//!
//! ```no_run
//! let ws = svlang_lint::Workspace::open(std::path::Path::new("."))?;
//! let checked = ws.check(&[])?;
//! for f in &checked.findings {
//!     let (file, line, col) = f.position_in(&checked.files);
//!     println!("{file}:{line}:{col}: {} {}", f.rule, f.message);
//! }
//! # Ok::<(), String>(())
//! ```
//!
//! The findings are the ones the command line reports for the project:
//! the same rule selection, configuration, waivers and baseline. Text set
//! with [`svlang::syntax::overlay`] stands in for files on disk, so a language
//! server checks what is in the editor.

use crate::{project, suppress, Finding, Session};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Editor-supplied configuration overrides (from VS Code / LSP settings).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorOptions {
    /// Explicit fallback or override config file path (`svlang.lint.configPath`).
    pub config_path: Option<PathBuf>,
    /// `"filesystemFirst"` (default: repo config wins) or `"editorFirst"` (editor settings win).
    pub config_preference: String,
    /// Only run automatic lint when a repo config file exists (`svlang.lint.requireConfig`).
    pub require_config: bool,
    /// Keep IEEE 1800 syntax & semantic compiler checks (`SV-CMP-001`) active when automatic linting is `off`.
    pub syntax_diagnostics: bool,
    /// Default/override rule preset (`svlang.lint.preset`).
    pub preset: Option<String>,
    /// Explicit rule IDs/names to enable (`svlang.lint.select`).
    pub select: Vec<String>,
    /// Explicit rule IDs/names to silence (`svlang.lint.ignore`).
    pub ignore: Vec<String>,
    /// Extra `.f` filelists (`svlang.compile.filelists`).
    pub filelists: Vec<String>,
    /// Extra preprocessor defines `"NAME"` or `"NAME=VAL"` (`svlang.compile.defines`).
    pub defines: Vec<String>,
    /// Extra `+incdir+` include directories (`svlang.compile.includeDirs`).
    pub include_dirs: Vec<String>,
}

/// A project folder with its configuration, waivers and baseline.
pub struct Workspace {
    root: PathBuf,
    config: Option<project::Config>,
    has_repo_config: bool,
    extra_args: Vec<String>,
    ignored: HashSet<String>,
    suppress: suppress::Suppress,
    session: Session,
    keep: HashSet<&'static str>,
    /// What loading the configuration and the waivers warned about.
    pub warnings: Vec<String>,
}

/// The result of [`Workspace::check`].
pub struct Checked {
    /// The file table the findings' locations index.
    pub files: Vec<String>,
    /// What is left after the configuration, waivers and baseline.
    pub findings: Vec<Finding>,
    /// How many findings waivers and the baseline took out.
    pub waived: usize,
}

/// The project config in `root` or a folder above it.
fn find_config(root: &Path) -> Option<PathBuf> {
    // `find` looks from the folder of a source: any name in `root` does.
    let probe = root.join("x.sv");
    let abs = if probe.is_relative() { std::env::current_dir().ok()?.join(probe) } else { probe };
    let mut dir = abs.parent();
    while let Some(d) = dir {
        if let Some(f) = project::NAMES.iter().map(|n| d.join(n)).find(|f| f.is_file()) {
            return Some(f);
        }
        dir = d.parent();
    }
    None
}

/// `.svh`, `.vh` and the like: text meant to be included, not compiled.
fn is_header(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e, "svh" | "vh" | "h" | "inc" | "svi"))
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

impl Workspace {
    /// Opens the project of folder `root`: the config found there or above
    /// (none is fine), the waiver file and the baseline next to it.
    pub fn open(root: &Path) -> Result<Workspace, String> {
        Self::open_with_preset(root, None)
    }

    /// Like [`Self::open`], using `default_preset` when the project config does
    /// not specify a `"preset"` (e.g., an LSP client passing `initializationOptions.preset`).
    pub fn open_with_preset(root: &Path, default_preset: Option<&str>) -> Result<Workspace, String> {
        let ed = EditorOptions { preset: default_preset.map(String::from), ..Default::default() };
        Self::open_with_editor(root, &ed)
    }

    /// Opens `root` merging repository `.svlang-lint` config with `editor` settings
    /// according to `editor.config_preference` (`"filesystemFirst"` vs `"editorFirst"`).
    pub fn open_with_editor(root: &Path, editor: &EditorOptions) -> Result<Workspace, String> {
        let mut warnings = Vec::new();
        let editor_first = editor.config_preference.eq_ignore_ascii_case("editorFirst");
        let repo_cfg_file = find_config(root);
        let has_repo_config = repo_cfg_file.is_some();
        let explicit_cfg = editor.config_path.clone().filter(|p| p.is_file());
        let chosen_file = if editor_first { explicit_cfg.or(repo_cfg_file) } else { repo_cfg_file.or(explicit_cfg) };
        let config = match chosen_file {
            Some(file) => {
                let cfg = project::Config::load(&file)?;
                warnings.extend(cfg.warnings.iter().map(|w| format!("{}: {w}", file.display())));
                Some(cfg)
            }
            None => None,
        };
        let ws_root = if has_repo_config {
            config
                .as_ref()
                .map(|c| c.root.clone())
                .filter(|r| !r.as_os_str().is_empty())
                .unwrap_or_else(|| root.to_path_buf())
        } else {
            root.to_path_buf()
        };
        let suppress = suppress::Suppress::load_in(&ws_root, config.as_ref(), None, None, false)?;
        warnings.extend(suppress.warnings.iter().cloned());
        let mut session = Session::new();
        if let Some(c) = &config {
            session
                .set_options(&c.options)
                .set_str_options(&c.str_options)
                .set_list_options(&c.list_options)
                .set_style(&c.style)
                .set_naming(&c.naming);
        }

        let cfg_preset = config.as_ref().and_then(|c| c.preset.as_deref());
        let ed_preset = editor.preset.as_deref().filter(|s| !s.is_empty());
        let preset = if editor_first || !has_repo_config { ed_preset.or(cfg_preset) } else { cfg_preset.or(ed_preset) };
        let selected = crate::rule_selection(preset, false)?;
        let apply_editor_rules = editor_first || !has_repo_config;
        let sel_set: HashSet<&str> = editor.select.iter().map(String::as_str).collect();
        let ign_set: HashSet<String> = if apply_editor_rules {
            editor.ignore.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        } else {
            HashSet::new()
        };

        let keep = crate::rules()
            .into_iter()
            .filter(|r| {
                if apply_editor_rules && (ign_set.contains(r.id) || ign_set.contains(r.name)) {
                    return false;
                }
                if apply_editor_rules
                    && !sel_set.is_empty()
                    && !editor_first
                    && editor.preset.is_none()
                    && config.is_none()
                {
                    return sel_set.contains(r.id) || sel_set.contains(r.name);
                }
                selected(r.id)
                    || config.as_ref().is_some_and(|c| c.selects(r.id, r.name))
                    || (apply_editor_rules && (sel_set.contains(r.id) || sel_set.contains(r.name)))
            })
            .map(|r| r.id)
            .collect();

        let mut extra_args = Vec::new();
        for inc in &editor.include_dirs {
            let p = Path::new(inc);
            let resolved = if p.is_relative() { ws_root.join(p) } else { p.to_path_buf() };
            extra_args.push(format!("+incdir+{}", resolved.display()));
        }
        for def in &editor.defines {
            let d = def.trim();
            if !d.is_empty() {
                extra_args.push(format!("+define+{d}"));
            }
        }
        if !has_repo_config || editor_first {
            for fl in &editor.filelists {
                let p = Path::new(fl);
                let resolved = if p.is_relative() { ws_root.join(p) } else { p.to_path_buf() };
                if resolved.is_file() {
                    extra_args.push("-F".to_string());
                    extra_args.push(resolved.display().to_string());
                }
            }
        }

        Ok(Workspace {
            root: ws_root,
            config,
            has_repo_config,
            extra_args,
            ignored: ign_set,
            suppress,
            session,
            keep,
            warnings,
        })
    }

    /// True when a repository config file (`.svlang-lint/config.py`, etc.) was discovered in or above `root`.
    pub fn has_repo_config(&self) -> bool {
        self.has_repo_config
    }

    /// The project's folder (the config's, when there is one).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The config file in use.
    pub fn config_file(&self) -> Option<&Path> {
        self.config.as_ref().map(|c| c.file.as_path())
    }

    /// Lints the project's sources, and each of `open` the project does not
    /// compile as a file of its own (every one of them when the config names
    /// no sources). Headers nothing compiled are left out: alone they do
    /// not parse.
    pub fn check(&self, open: &[PathBuf]) -> Result<Checked, String> {
        self.check_inner(open, false)
    }

    /// Runs only IEEE 1800-2023 syntax and semantic compiler checks (`SV-CMP-001`)
    /// without invoking static lint rules (used in Mode 1 when automatic linting is off).
    pub fn check_compiler_only(&self, open: &[PathBuf]) -> Result<Checked, String> {
        self.check_inner(open, true)
    }

    fn check_inner(&self, open: &[PathBuf], compiler_only: bool) -> Result<Checked, String> {
        let mut combined_args: Vec<String> = self.config.as_ref().map_or_else(Vec::new, |c| c.compile_args.clone());
        combined_args.extend(self.extra_args.iter().cloned());
        let args: &[String] = &combined_args;
        let cwd = std::env::current_dir().unwrap_or_default();
        let list =
            svlang::source::FileList::read_args(args, &cwd, svlang::source::Dialect::Any, &svlang::source::ProcessEnv);
        let standalone_mode = !self.has_repo_config && list.sources.is_empty();
        // Include folders and defines, for files parsed on their own.
        let mut shared: Vec<String> =
            list.include_dirs.iter().map(|d| format!("+incdir+{}", d.value.display())).collect();
        shared.extend(list.effective_defines().into_iter().map(|(n, v)| match v {
            Some(v) => format!("+define+{n}={v}"),
            None => format!("+define+{n}"),
        }));
        let mut trees = Vec::new();
        if !list.sources.is_empty() {
            trees = svlang::semantic::parse_args(args).map_err(|e| e.to_string())?.0;
        }
        let compiled: HashSet<PathBuf> = trees.iter().flat_map(|t| t.files.iter().map(|f| canonical(f))).collect();
        let mut seen = HashSet::new();
        let alone: Vec<String> = open
            .iter()
            .filter(|p| !is_header(p) && !compiled.contains(&canonical(p)) && seen.insert(canonical(p)))
            .map(|p| p.display().to_string())
            .collect();
        if !alone.is_empty() {
            shared.extend(alone);
            trees.extend(svlang::semantic::parse_args(&shared).map_err(|e| e.to_string())?.0);
        }
        if trees.is_empty() {
            return Ok(Checked { files: Vec::new(), findings: Vec::new(), waived: 0 });
        }
        let design = svlang_model::design_of(&trees, 0.0);
        self.session.reset_waivers();
        let (files, mut findings) = self.session.lint_all(&design, &trees, |id| {
            if compiler_only {
                id == "SV-CMP-001" && !self.ignored.contains("SV-CMP-001")
            } else {
                self.keep.contains(id)
            }
        });
        if standalone_mode {
            filter_standalone_compiler_noise(&trees, &files, &mut findings);
        }
        if !self.ignored.is_empty() {
            findings.retain(|f| !self.ignored.contains(f.rule) && !self.ignored.contains(f.name));
        }
        let inline = self.session.inline_waived();
        crate::fix::guard_macro_calls(&mut findings);
        // The editor shows the user's code: never the UVM library's findings.
        let sources: Vec<std::path::PathBuf> = trees.iter().filter_map(|t| t.files.first().cloned()).collect();
        crate::library::Library::detect(&sources).hide(
            &mut findings,
            &files,
            self.config.as_ref().is_some_and(|c| c.reports_library_macros()),
        );
        if let Some(c) = &self.config {
            c.apply(&mut findings, &files);
        }
        let waived = inline + self.suppress.apply_in_session(&self.session, &mut findings, &files);
        crate::expansions::name_macros(&mut findings, &files);
        Ok(Checked { files, findings, waived })
    }
}

/// Suppresses missing-external-context `SV-CMP-001` noise when standalone files are
/// opened in the editor without a `.svlang-lint/config.py` or `.f` filelist, while
/// preserving all genuine syntax errors and self-contained IEEE 1800 semantic errors.
fn filter_standalone_compiler_noise(trees: &[svlang::syntax::Tree], files: &[String], findings: &mut Vec<Finding>) {
    use svlang::syntax::SyntaxKind::*;

    let mut declared_pkgs: HashSet<String> = HashSet::new();
    let mut declared_classes: HashSet<String> = HashSet::new();
    for t in trees {
        for n in t.preorder() {
            match t.kind(n) {
                PackageDecl => {
                    if let Some(nm) = t.child(n, Name) {
                        declared_pkgs.insert(t.bare_text(nm));
                    }
                }
                ClassDecl => {
                    let hdr = t.child(n, Header);
                    if let Some(nm) = t.child(n, Name).or_else(|| hdr.and_then(|h| t.child(h, Name))) {
                        declared_classes.insert(t.bare_text(nm));
                    }
                }
                _ => {}
            }
        }
    }

    // Per standalone file: track lines with unknown macros and whether unresolved external imports/includes/bases exist.
    let mut external_deps_files: HashSet<PathBuf> = HashSet::new();
    let mut macro_err_lines: HashSet<(PathBuf, u32)> = HashSet::new();

    for t in trees {
        let Some(main_file) = t.files.first().map(|p| canonical(p)) else { continue };
        let mut has_ext = false;
        for d in &t.pp_diagnostics {
            if d.message.starts_with("cannot find include file ") || d.message.starts_with("unknown macro") {
                has_ext = true;
                if d.message.starts_with("unknown macro") {
                    if let Some(src) = t.sources.get(d.origin.file as usize) {
                        let off = (d.origin.offset as usize).min(src.len());
                        let line = src[..off].bytes().filter(|&b| b == b'\n').count() as u32 + 1;
                        macro_err_lines.insert((main_file.clone(), line));
                        macro_err_lines.insert((main_file.clone(), line + 1));
                    }
                }
            }
        }
        for n in t.preorder() {
            match t.kind(n) {
                ImportDecl => {
                    let txt = t.bare_text(n);
                    let body = txt.strip_prefix("import").unwrap_or(&txt).trim().trim_end_matches(';');
                    for item in body.split(',') {
                        let pkg = item.split("::").next().unwrap_or("").trim();
                        if !pkg.is_empty() && !declared_pkgs.contains(pkg) {
                            has_ext = true;
                        }
                    }
                }
                Extends => {
                    if let Some(dt) = t.child(n, DataType).or_else(|| t.child(n, Name)) {
                        let raw = t.bare_text(dt);
                        let base = raw.split('#').next().unwrap_or(&raw).rsplit("::").next().unwrap_or(&raw).trim();
                        if !base.is_empty() && !declared_classes.contains(base) {
                            has_ext = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if has_ext {
            external_deps_files.insert(main_file);
        }
    }

    findings.retain(|f| {
        if f.rule != "SV-CMP-001" {
            return true;
        }
        let msg = f.message.as_str();
        if msg.starts_with("cannot find include file ") || msg.starts_with("unknown macro") {
            return false;
        }
        let (fname, line, _) = f.position_in(files);
        let cpath = canonical(Path::new(fname));
        if macro_err_lines.contains(&(cpath.clone(), line)) {
            return false;
        }
        if external_deps_files.contains(&cpath)
            && (msg.starts_with("unknown package ")
                || msg.starts_with("unknown class")
                || msg.starts_with("unknown type ")
                || msg.starts_with("no member named ")
                || msg.starts_with("use of undeclared identifier ")
                || msg.starts_with("undeclared identifier ")
                || msg.starts_with("extern constraint ")
                || msg.starts_with("could not resolve hierarchical path name ")
                || msg.starts_with("unknown base covergroup "))
        {
            return false;
        }
        true
    });
}

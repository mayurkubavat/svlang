//! A lint session: the settings one run of the rules reads, held in a value
//! instead of in the process.
//!
//! ```no_run
//! # let (design, trees): (svlang_model::Design, Vec<svlang::syntax::Tree>) = todo!();
//! let mut session = svlang_lint::Session::new();
//! session.add_tops(&["tb_top".to_string()]).set_skip_libraries(true);
//! let (files, findings) = session.lint_all(&design, &trees, |_| true);
//! ```
//!
//! Two sessions with different settings can run at the same time in one
//! process (an editor with two workspaces, a test harness, a host
//! language): each run hands its settings to the threads it starts.
//!
//! The free functions ([`crate::options::set`], [`crate::add_tops`],
//! [`crate::lint_all`], ...) work on the process's own session, as the
//! command line does: one project per process.

use crate::options::{NamingOverride, StyleValue};
use crate::waivers::WaiverState;
use crate::Finding;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use svlang_model::Design;

/// What the rules of one run read besides the sources.
#[derive(Clone, Debug, Default)]
pub(crate) struct Settings {
    /// `rules.<RULE>.options`: (rule ID, key) -> integer/bool value.
    pub(crate) options: HashMap<(String, String), i64>,
    /// `rules.<RULE>.options`: (rule ID, key) -> string value.
    pub(crate) str_options: HashMap<(String, String), String>,
    /// `rules.<RULE>.options`: (rule ID, key) -> string-list value.
    pub(crate) list_options: HashMap<(String, String), Vec<String>>,
    /// The `style` block.
    pub(crate) style: HashMap<String, StyleValue>,
    /// The `naming` block, by target.
    pub(crate) naming: HashMap<String, NamingOverride>,
    /// `-top`: elaboration starts from these only.
    pub(crate) tops: Vec<String>,
    /// `--skip-libraries`: report nothing in `-v`/`-y` files.
    pub(crate) skip_libraries: bool,
    /// `--history`: allow `SVL-MNT-027` / `SVL-MNT-028` rules to read git log history.
    pub(crate) history: bool,
    /// Per-session inline waiver statistics and matched comment lines.
    pub(crate) waiver_state: Arc<Mutex<WaiverState>>,
}

/// The process's session: what the free setter functions change.
static PROCESS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

thread_local! {
    /// The settings of the run this thread works for.
    static CURRENT: RefCell<Option<Arc<Settings>>> = const { RefCell::new(None) };
}

/// The process session's settings as they are now.
pub(crate) fn process() -> Arc<Settings> {
    if let Some(p) = PROCESS.read().ok().and_then(|p| p.clone()) {
        return p;
    }
    PROCESS.write().ok().map(|mut p| p.get_or_insert_with(Default::default).clone()).unwrap_or_default()
}

/// Changes the process session's settings.
pub(crate) fn change_process(f: impl FnOnce(&mut Settings)) {
    if let Ok(mut p) = PROCESS.write() {
        f(Arc::make_mut(p.get_or_insert_with(Default::default)));
    }
}

/// Makes `settings` this thread's (a thread of that run), or none.
pub(crate) fn bind(settings: Option<Arc<Settings>>) {
    CURRENT.with(|c| *c.borrow_mut() = settings);
}

/// Reads the settings in force on this thread: its run's, else the
/// process's (outside a run: the command line's reports and commands).
pub(crate) fn with<R>(f: impl FnOnce(&Settings) -> R) -> R {
    CURRENT.with(|c| match c.borrow().as_deref() {
        Some(s) => f(s),
        None => f(&process()),
    })
}

/// The settings of one or more lint runs.
#[derive(Clone, Debug, Default)]
pub struct Session {
    settings: Settings,
}

impl Session {
    /// A session with every setting at its default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rule options: (rule ID or name, key, value).
    pub fn set_options(&mut self, options: &[(String, String, i64)]) -> &mut Self {
        self.settings.options = crate::options::resolved(options);
        self
    }

    /// String rule options: (rule ID or name, key, value).
    pub fn set_str_options(&mut self, options: &[(String, String, String)]) -> &mut Self {
        self.settings.str_options = crate::options::resolved_map(options);
        self
    }

    /// String-list rule options: (rule ID or name, key, values).
    pub fn set_list_options(&mut self, options: &[(String, String, Vec<String>)]) -> &mut Self {
        self.settings.list_options = crate::options::resolved_map(options);
        self
    }

    /// The `style` block: (key, value).
    pub fn set_style(&mut self, style: &[(String, StyleValue)]) -> &mut Self {
        self.settings.style = style.iter().cloned().collect();
        self
    }

    /// The `naming` block: (target, settings).
    pub fn set_naming(&mut self, naming: &[(String, NamingOverride)]) -> &mut Self {
        self.settings.naming = naming.iter().cloned().collect();
        self
    }

    /// Adds top modules (`-top`): elaboration starts from them only.
    pub fn add_tops(&mut self, tops: &[String]) -> &mut Self {
        self.settings.tops.extend(tops.iter().cloned());
        self
    }

    /// Reports nothing in library files (`-v`, `-y`).
    pub fn set_skip_libraries(&mut self, on: bool) -> &mut Self {
        self.settings.skip_libraries = on;
        self
    }

    /// Enables or disables git-history rules (`--history`, `SVL-MNT-027` / `SVL-MNT-028`) for this session.
    pub fn set_history(&mut self, on: bool) -> &mut Self {
        self.settings.history = on;
        self
    }

    /// How many findings inline waiver comments have dropped in this session.
    pub fn inline_waived(&self) -> usize {
        self.settings.waiver_state.lock().map_or(0, |s| s.inline_waived)
    }

    /// Clears inline waiver counters accumulated in earlier runs of this session.
    pub fn reset_waivers(&self) {
        if let Ok(mut s) = self.settings.waiver_state.lock() {
            *s = WaiverState::default();
        }
    }

    pub(crate) fn waiver_state(&self) -> &Mutex<WaiverState> {
        &self.settings.waiver_state
    }

    /// [`crate::lint`] under this session's settings.
    pub fn lint(&self, design: &Design, keep: impl Fn(&str) -> bool) -> Vec<Finding> {
        self.lint_all(design, &[], keep).1
    }

    /// [`crate::lint_all`] under this session's settings.
    pub fn lint_all(
        &self,
        design: &Design,
        trees: &[svlang::syntax::Tree],
        keep: impl Fn(&str) -> bool,
    ) -> (Vec<String>, Vec<Finding>) {
        crate::lint_from(Arc::new(self.settings.clone()), crate::Source::Ready(design), trees, keep).1
    }

    /// [`crate::lint_built`] under this session's settings.
    pub fn lint_built(
        &self,
        build: impl FnOnce() -> Design,
        trees: &[svlang::syntax::Tree],
        keep: impl Fn(&str) -> bool,
    ) -> (Design, (Vec<String>, Vec<Finding>)) {
        let settings = Arc::new(self.settings.clone());
        let (d, out) = crate::lint_from(settings, crate::Source::Build(Box::new(build)), trees, keep);
        (d.expect("a built design"), out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_sessions_isolate_options_history_and_waivers() {
        let dir = std::env::temp_dir().join(format!("svlang-lint-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path_a = dir.join("a.sv");
        let path_b = dir.join("b.sv");
        let src_a = "module m;\n  int a; // padding to exceed thirty columns!\n  initial a = 5; // svlang-lint:waive SVL-STY-008\nendmodule\n";
        let src_b = "module n;\n  int a; // padding to exceed thirty columns!\n  initial a = 6;\nendmodule\n";
        std::fs::write(&path_a, src_a).unwrap();
        std::fs::write(&path_b, src_b).unwrap();

        let tree_a = svlang::syntax::parse_file(&path_a, Default::default());
        let tree_b = svlang::syntax::parse_file(&path_b, Default::default());
        let design_a = svlang_model::design_of(std::slice::from_ref(&tree_a), 0.0);
        let design_b = svlang_model::design_of(std::slice::from_ref(&tree_b), 0.0);

        let mut sess_a = Session::new();
        sess_a.set_options(&[("SVL-STY-013".into(), "max_columns".into(), 30)]).set_history(true);
        let mut sess_b = Session::new();
        sess_b.set_options(&[("SVL-STY-013".into(), "max_columns".into(), 120)]).set_history(false);

        let (res_a, res_b) = std::thread::scope(|s| {
            let ha = s.spawn(|| {
                let (_, f) = sess_a.lint_all(&design_a, std::slice::from_ref(&tree_a), |id| {
                    matches!(id, "SVL-STY-013" | "SVL-STY-008")
                });
                (f, sess_a.inline_waived())
            });
            let hb = s.spawn(|| {
                let (_, f) = sess_b.lint_all(&design_b, std::slice::from_ref(&tree_b), |id| {
                    matches!(id, "SVL-STY-013" | "SVL-STY-008")
                });
                (f, sess_b.inline_waived())
            });
            (ha.join().unwrap(), hb.join().unwrap())
        });
        let _ = std::fs::remove_dir_all(&dir);

        assert!(res_a.0.iter().any(|f| f.rule == "SVL-STY-013"), "sess_a max_columns=30 must trigger SVL-STY-013");
        assert!(!res_a.0.iter().any(|f| f.rule == "SVL-STY-008"), "sess_a inline waiver must drop SVL-STY-008");
        assert_eq!(res_a.1, 1, "sess_a inline_waived must equal 1");

        assert!(
            !res_b.0.iter().any(|f| f.rule == "SVL-STY-013"),
            "sess_b max_columns=120 must not trigger SVL-STY-013"
        );
        assert!(res_b.0.iter().any(|f| f.rule == "SVL-STY-008"), "sess_b has no waiver for SVL-STY-008");
        assert_eq!(res_b.1, 0, "sess_b inline_waived must remain 0");
    }
}

//! UVM-CFG-013 (ConfigDbGetInRunPhaseLoop): Flags `uvm_config_db#(...)::get(...)`
//! calls inside loops in runtime task phases (`run_phase`, `main_phase`, etc.)
//! or sequence `body()` tasks, where repeated regex/glob table lookups degrade
//! simulation throughput.

use crate::is_task_phase;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;

pub(crate) fn config_db_get_in_run_phase_loop(c: &Cst, out: &mut Sink) {
    for e in c.nodes(CallExpr) {
        let Some(t0) = c.first_tok(e) else { continue };
        if is_library_tok(c, t0) {
            continue;
        }
        let Some(callee) = c.tree.children(e).next() else { continue };
        let callee_txt = c.text(callee);
        if !callee_txt.contains("uvm_config_db") || !callee_txt.trim_end().ends_with("::get") {
            continue;
        }
        let mut in_loop = false;
        let mut routine_name: Option<String> = None;
        for anc in std::iter::successors(c.parent(e), |&a| c.parent(a)) {
            match c.kind(anc) {
                LoopStmt | DoWhileStmt => in_loop = true,
                TaskDecl => {
                    routine_name = Some(crate::support::slang_ast::last_name(c, anc));
                    break;
                }
                FunctionDecl => break,
                _ => {}
            }
        }
        let Some(rname) = routine_name.filter(|_| in_loop) else { continue };
        let bare = rname.rsplit("::").next().unwrap_or(&rname);
        if is_task_phase(bare) || bare == "body" || bare == "pre_body" || bare == "run_test" {
            out.add(
                "UVM-CFG-013",
                "ConfigDbGetInRunPhaseLoop",
                Severity::Warning,
                format!(
                    "'uvm_config_db::get()' called inside a loop in task '{bare}'. Each lookup scans the global resource table and evaluates path globs; cache configuration fields once in 'build_phase' or before entering the loop."
                ),
                c.loc(t0),
            );
        }
    }
}

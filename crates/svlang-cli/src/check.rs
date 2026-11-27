//! Oracle / gate inspection modes for `svlang check` (replacing the former
//! `dv-build` tool): `--dump`, `--cst`, `--symbols` (`--types`, `--values`,
//! `--specs`), `--hier`, `--netlist`, `--dialect`, and `--quiet`.

use std::process::ExitCode;

use svlang::source::{Dialect, FileList, ProcessEnv, Severity};
use svlang::syntax::{Options, Tree};

struct Diag {
    file: String,
    line: u32,
    column: u32,
    message: String,
}

/// Returns `true` when `args` asks for one of the oracle / gate inspection
/// modes (`--dump`, `--symbols`, `--netlist`, `--quiet`, `--dialect`, ...).
pub fn is_oracle_mode(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--dump"
                | "--cst"
                | "--quiet"
                | "--symbols"
                | "--types"
                | "--values"
                | "--specs"
                | "--hier"
                | "--netlist"
                | "--dialect"
        )
    })
}

pub fn run_oracle(raw_args: &[String]) -> ExitCode {
    let mut dialect = Dialect::Any;
    let (mut dump, mut quiet, mut symbols, mut types, mut values) = (false, false, false, false, false);
    let mut specs = false;
    let mut cst = false;
    let mut hier = false;
    let mut emit_netlist = false;
    let mut blast_memories: Option<usize> = None;
    let mut allow_four_state_ops = false;
    let mut skip_initial_checks = false;
    let mut isunknown_is_false = false;
    let mut tristate_checks = false;
    let mut single_unit = false;
    let mut separate_units = false;
    let mut args = Vec::new();
    let mut it = raw_args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dump" => dump = true,
            "--cst" => cst = true,
            "--quiet" => quiet = true,
            "--symbols" => symbols = true,
            "--hier" => hier = true,
            "--netlist" => emit_netlist = true,
            "--allow-four-state-ops" | "--four-state" => allow_four_state_ops = true,
            "--skip-initial-checks" => skip_initial_checks = true,
            "--isunknown-is-false" => isunknown_is_false = true,
            "--tristate-checks" => tristate_checks = true,
            "--single-unit" => single_unit = true,
            "--separate-units" => separate_units = true,
            "--blast-memories" => {
                let Some(val_s) = it.next() else {
                    eprintln!("error: --blast-memories expects a maximum depth integer");
                    return ExitCode::from(2);
                };
                match val_s.parse::<usize>() {
                    Ok(n) => blast_memories = Some(n),
                    Err(_) => {
                        eprintln!("error: --blast-memories expects a non-negative integer, got {val_s:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--values" => {
                symbols = true;
                values = true;
            }
            "--types" => {
                symbols = true;
                types = true;
            }
            "--specs" => {
                symbols = true;
                specs = true;
            }
            "--dialect" => {
                dialect = match it.next().map(String::as_str) {
                    Some("vcs") => Dialect::Vcs,
                    Some("xcelium") => Dialect::Xcelium,
                    Some("any") => Dialect::Any,
                    other => {
                        eprintln!("error: --dialect takes any, vcs or xcelium, not {other:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            "-h" | "--help" => {
                println!(
                    "usage: svlang check [--json] [--dialect any|vcs|xcelium] [--single-unit|--separate-units] \
                     [--dump] [--cst] [--symbols] [--types] [--values] [--specs] [--hier] [--netlist] \
                     [--blast-memories N] [--four-state] [--skip-initial-checks] [--isunknown-is-false] \
                     [--quiet] <options> FILE..."
                );
                return ExitCode::SUCCESS;
            }
            _ => args.push(a.clone()),
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let list = FileList::read_args(&args, &cwd, dialect, &ProcessEnv);
    let mut diags = Vec::new();
    for d in &list.diagnostics {
        if d.severity == Severity::Error {
            diags.push(Diag {
                file: d.origin.file.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                line: d.origin.line,
                column: 0,
                message: d.message.clone(),
            });
        }
    }
    if list.sources.is_empty() {
        eprintln!("error: no sources given");
        return ExitCode::from(2);
    }
    let opts = Options {
        include_dirs: list.include_dirs.iter().map(|d| d.value.clone()).collect(),
        defines: list
            .effective_defines()
            .into_iter()
            .map(|(n, v)| match v {
                Some(v) => format!("{n}={v}"),
                None => n,
            })
            .collect(),
        ..Options::default()
    };
    for s in &list.sources {
        if !s.value.exists() {
            eprintln!("error: cannot read {}", s.value.display());
            return ExitCode::from(2);
        }
    }
    let paths: Vec<_> = list.sources.iter().map(|s| s.value.clone()).collect();
    let mut trees = if single_unit {
        vec![svlang::syntax::parse_unit(&paths, opts.clone())]
    } else if separate_units {
        svlang::syntax::parse_files_isolated(&paths, opts.clone())
    } else {
        svlang::syntax::parse_files(&paths, opts.clone())
    };
    svlang::semantic::library::add_library_trees(&mut trees, &list, &opts);
    let tops: Vec<String> = list.top.iter().map(|t| t.value.clone()).collect();
    let params: Vec<(String, String)> = list.param_overrides.iter().map(|p| p.value.clone()).collect();
    let comp = svlang::Compilation::from_trees_with_tops(trees, tops).with_params(params);
    let (checked, elab) = comp.check();
    let hier_lines = elab.hierarchy;
    for d in checked {
        diags.push(Diag { file: d.file, line: d.line, column: d.column, message: d.message });
    }
    if !quiet {
        for d in &diags {
            println!("{}:{}:{}: error: {}", d.file, d.line, d.column, d.message);
        }
    }
    if dump {
        for t in comp.trees() {
            print_dump(t);
        }
    }
    if cst {
        for t in comp.trees() {
            print_cst(t, t.root, 0);
        }
    }
    if hier {
        for l in hier_lines {
            println!("{l}");
        }
    } else if symbols {
        let scopes = comp.scopes();
        let (tys, vals) =
            if types || values { svlang::semantic::symbol_info(comp.trees(), &scopes) } else { Default::default() };
        let mut lines = scopes.dump_full(types.then_some(tys.as_slice()), values.then_some(vals.as_slice()));
        if specs {
            lines.extend(svlang::semantic::spec_symbol_lines(comp.trees(), &scopes, types, values));
            lines.sort();
        }
        for l in lines {
            println!("{l}");
        }
    }
    if emit_netlist && diags.is_empty() {
        let cfg = svlang::LowerConfig {
            top: None,
            blast_memories_up_to: blast_memories,
            allow_four_state_ops,
            skip_initial_checks,
            isunknown_is_false,
            tristate_contention_checks: tristate_checks,
            tristate_floating_checks: tristate_checks,
            ..svlang::LowerConfig::default()
        };
        match comp.lower_netlist(&cfg) {
            svlang::LowerOutcome::Lowered(nl) => {
                print!("{}", nl.dump());
            }
            svlang::LowerOutcome::Unsupported(ds) | svlang::LowerOutcome::Error(ds) => {
                for d in ds {
                    if !quiet {
                        println!("{d}");
                    }
                    diags.push(Diag { file: d.file, line: d.loc.line, column: d.loc.column, message: d.message });
                }
            }
        }
    }
    println!("errors {}", diags.len());
    if diags.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_cst(t: &Tree, n: svlang::syntax::NodeId, depth: usize) {
    let own: Vec<&str> = t
        .node(n)
        .children
        .iter()
        .filter_map(|c| match c {
            svlang::syntax::Child::Token(i) => Some(&*t.tokens[*i as usize].text),
            svlang::syntax::Child::Node(_) => None,
        })
        .collect();
    println!("{}{:?} {}", "  ".repeat(depth), t.kind(n), own.join(" "));
    for c in t.children(n) {
        print_cst(t, c, depth + 1);
    }
}

fn print_dump(t: &Tree) {
    let entries = svlang::syntax::outline(t);
    let mut depth_of = std::collections::HashMap::new();
    let mut parent = vec![u32::MAX; t.nodes.len()];
    for n in t.preorder() {
        for c in t.children(n) {
            parent[c.0 as usize] = n.0;
        }
    }
    for e in &entries {
        let mut d = 0usize;
        let mut p = parent[e.node.0 as usize];
        while p != u32::MAX {
            if let Some(&pd) = depth_of.get(&p) {
                d = pd + 1;
                break;
            }
            p = parent[p as usize];
        }
        depth_of.insert(e.node.0, d);
        println!("{}{} {}", "  ".repeat(d), e.kind, e.name);
    }
}

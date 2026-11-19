//! `svlang`: the command line of the svlang SystemVerilog front end.
//!
//!     svlang <command> [options] <compile arguments>
//!
//! The compile arguments are what a simulator takes: source files, `-f`/`-F`
//! file lists, `+incdir+DIR`, `+define+NAME=VALUE`, `-y`/`-v` libraries,
//! `-top MODULE` and `-G NAME=VALUE`. See [`USAGE`] for the commands.
//!
//! Exit status: 0 success, 1 the design has errors or cannot be written in the
//! requested form, 2 bad usage or unreadable input.

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;

use svlang::netlist::{PropertyOptions, PropertyOutcome, ValueKind};
use svlang::{Compilation, Diagnostic, DiagnosticStage, Driver, LowerConfig, LowerOutcome, Netlist, Severity};

const USAGE: &str = "\
svlang: a SystemVerilog (IEEE 1800-2023) front end

usage: svlang <command> [options] <compile arguments>

commands:
  check      report every error and warning; exit 1 when there are errors
  preprocess print the source after macro expansion, includes and conditionals
  hier       print the elaborated instance hierarchy and parameter values
  cst        print the syntax tree of every source file
  netlist    lower the design to a word-level RTL netlist and print it
  btor2      write the design as a Btor2 transition system
  aiger      write the design as an and-inverter graph in AIGER
  yosys      write the design as a Yosys JSON netlist (read_json)
  stages     run parse, elaborate, lower and Btor2 in turn; print as JSON how far
             the design got, the time each stage took and what stopped it
  help       print this text
  version    print the version

compile arguments (as a simulator takes them):
  FILE...  -f LIST  -F LIST  +incdir+DIR  +define+NAME[=VALUE]
  -y DIR  -v FILE  -top MODULE  -G NAME=VALUE  --single-unit  --separate-units

options:
  --json                  check: print the diagnostics as a JSON array
  -o, --output FILE       preprocess, netlist, btor2, aiger, yosys: write to FILE, not stdout
  --ascii                 aiger: write the ASCII form (.aag), not binary
  --trace-outputs         btor2: add the section that names combinational nets
  --properties            netlist, btor2, aiger, yosys: compile concurrent properties
                          (assert/assume/cover property) into checks
  --skip-cycles N         with --properties: do not check attempts that start in
                          the first N clock ticks (the length of reset)
  --covers                btor2, aiger: write covers as reachable bad states
  --clock NAME[:HALF[:PHASE]]  btor2, aiger: a clock of a design with several: the
                          input NAME is low then high for HALF model steps each
                          (default 1), its wave moved PHASE steps later (repeatable)
  --both-edges            btor2, aiger: accept both edges of the one clock
  --blast-memories N      netlist, btor2, aiger, yosys: expand memories of depth <= N
  --four-state            netlist, btor2, aiger, yosys: keep ===, !== and $isunknown
  --skip-initial-checks   leave out initial-block assertions over signals
  --isunknown-is-false    lower $isunknown as 0 (two-state only)
  --cut PATH              netlist, btor2, aiger, yosys: cut the signal at PATH (repeatable)
  --black-box MODULE      netlist, btor2, aiger, yosys: do not lower MODULE (repeatable)
  --black-box-instance PATH   the same for one instance (repeatable)

exit status: 0 success, 1 errors in the design, 2 bad usage
";

/// The command's own options; everything else is a compile argument.
#[derive(Default)]
struct Opts {
    json: bool,
    output: Option<String>,
    ascii: bool,
    trace_outputs: bool,
    properties: bool,
    skip_cycles: u32,
    covers: bool,
    both_edges: bool,
    clocks: Vec<svlang::btor2::Clock>,
    lower: LowerConfig,
    compile_args: Vec<String>,
}

/// A usage error: printed with a pointer to `svlang help`, exit status 2.
struct Usage(String);

fn parse(args: &[String]) -> Result<Opts, Usage> {
    let mut o = Opts::default();
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<'_, String>| {
        it.next().cloned().ok_or_else(|| Usage(format!("{name} needs a value")))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => o.json = true,
            "--ascii" => o.ascii = true,
            "--trace-outputs" => o.trace_outputs = true,
            "--properties" => o.properties = true,
            "--covers" => o.covers = true,
            "--both-edges" => o.both_edges = true,
            "--clock" => {
                let v = value(a, &mut it)?;
                let mut parts = v.split(':');
                let input = parts.next().unwrap_or_default().to_owned();
                let mut number = |what: &str, default: u32| match parts.next() {
                    None => Ok(default),
                    Some(n) => n.parse().map_err(|_| Usage(format!("--clock {v}: {what} must be a number"))),
                };
                let (half_period, phase) = (number("HALF", 1)?, number("PHASE", 0)?);
                if input.is_empty() || half_period == 0 {
                    return Err(Usage(format!("--clock {v}: expected NAME[:HALF[:PHASE]] with HALF at least 1")));
                }
                o.clocks.push(svlang::btor2::Clock { input, half_period, phase });
            }
            "--skip-cycles" => {
                let v = value(a, &mut it)?;
                o.skip_cycles = v.parse().map_err(|_| Usage(format!("--skip-cycles needs a number, not '{v}'")))?;
            }
            "--four-state" => o.lower.allow_four_state_ops = true,
            "--skip-initial-checks" => o.lower.skip_initial_checks = true,
            "--isunknown-is-false" => o.lower.isunknown_is_false = true,
            "-o" | "--output" => o.output = Some(value(a, &mut it)?),
            "--blast-memories" => {
                let v = value(a, &mut it)?;
                o.lower.blast_memories_up_to =
                    Some(v.parse().map_err(|_| Usage(format!("--blast-memories needs a number, not '{v}'")))?);
            }
            "--cut" => o.lower.cut_points.push(value(a, &mut it)?),
            "--black-box" => o.lower.black_box_modules.push(value(a, &mut it)?),
            "--black-box-instance" => o.lower.black_box_instances.push(value(a, &mut it)?),
            _ => o.compile_args.push(a.clone()),
        }
    }
    Ok(o)
}

fn compile(o: &Opts) -> Result<Compilation, Usage> {
    if o.compile_args.is_empty() {
        return Err(Usage("no source files given".into()));
    }
    let mut d = Driver::new();
    d.add_args(&o.compile_args).map_err(|e| Usage(e.to_string()))?;
    d.compile().map_err(|e| Usage(e.to_string()))
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn diagnostics_json(diags: &[Diagnostic]) -> String {
    let mut out = String::from("[");
    for (i, d) in diags.iter().enumerate() {
        let stage = match d.stage {
            DiagnosticStage::FileList => "filelist",
            DiagnosticStage::Preprocessor => "preprocessor",
            DiagnosticStage::Parser => "parser",
            DiagnosticStage::Semantic => "semantic",
        };
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let _ = write!(
            out,
            "{}\n  {{\"file\": {}, \"line\": {}, \"column\": {}, \"stage\": \"{stage}\", \"severity\": \"{severity}\", \"message\": {}}}",
            if i == 0 { "" } else { "," },
            json_string(&d.file),
            d.line,
            d.column,
            json_string(&d.message)
        );
    }
    out.push_str(if diags.is_empty() { "]\n" } else { "\n]\n" });
    out
}

/// Writes `data` to the `-o` file, or to stdout.
fn emit(o: &Opts, data: &[u8]) -> Result<(), Usage> {
    match &o.output {
        Some(path) => std::fs::write(path, data).map_err(|e| Usage(format!("cannot write {path}: {e}"))),
        None => {
            let _ = std::io::stdout().write_all(data);
            Ok(())
        }
    }
}

/// The design's netlist, or its diagnostics on stderr and `None`.
fn lower(comp: &Compilation, o: &Opts) -> Option<Netlist> {
    let (diags, what) = match comp.lower_netlist(&o.lower) {
        LowerOutcome::Lowered(mut nl) => {
            if o.properties {
                compile_properties(&mut nl, o);
            }
            return Some(nl);
        }
        LowerOutcome::Unsupported(d) => (d, "unsupported"),
        LowerOutcome::Error(d) => (d, "error"),
    };
    for d in &diags {
        eprintln!("{}:{}:{}: {what}: {}", d.file, d.loc.line, d.loc.column, d.message);
    }
    eprintln!("svlang: the design was not lowered ({} {what}(s))", diags.len());
    None
}

/// Compiles the concurrent properties, and says on stderr which were left as they are.
fn compile_properties(nl: &mut Netlist, o: &Opts) {
    let mut opts = PropertyOptions::default();
    opts.skip_cycles = o.skip_cycles;
    let outcomes = nl.compile_properties(&opts);
    let mut compiled = 0;
    for (i, outcome) in outcomes.iter().enumerate() {
        match outcome {
            PropertyOutcome::Compiled { .. } | PropertyOutcome::Liveness { .. } => compiled += 1,
            PropertyOutcome::Unsupported(why) => eprintln!("note: property {i} not compiled: {why}"),
        }
    }
    eprintln!("note: {compiled} of {} concurrent properties compiled", outcomes.len());
}

fn btor2_options(o: &Opts) -> svlang::btor2::Options {
    let mut opts = svlang::btor2::Options::default();
    opts.trace_outputs = o.trace_outputs;
    opts.covers = o.covers;
    opts.both_edges = o.both_edges;
    opts.clocks.clone_from(&o.clocks);
    opts
}

fn notes(notes: &[String]) {
    for n in notes {
        eprintln!("note: {n}");
    }
}

/// The highest resident memory of this process so far, in kB (Linux; `None` elsewhere).
fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| l.strip_prefix("VmHWM:")?.split_whitespace().next()?.parse().ok())
}

/// `svlang stages`: one JSON object saying how far the design got through parse,
/// elaborate, lower and Btor2, how long each took, and the diagnostics of the stage
/// that stopped it. The second value is whether every stage passed.
fn stages(o: &Opts) -> Result<(String, bool), Usage> {
    use std::time::Instant;
    let mut times: Vec<(&str, f64)> = Vec::new();
    // (stage, kind, file, line, column, message)
    let mut found: Vec<(&str, &str, String, u32, u32, String)> = Vec::new();
    let mut blocked: Option<&str> = None;
    let mut sizes = String::new();

    let t = Instant::now();
    let comp = compile(o)?;
    times.push(("parse", t.elapsed().as_secs_f64()));

    let t = Instant::now();
    let (diags, elab) = comp.check();
    times.push(("elaborate", t.elapsed().as_secs_f64()));
    for d in diags.iter().filter(|d| d.severity == Severity::Error) {
        let stage = if d.stage == DiagnosticStage::Semantic { "elaborate" } else { "parse" };
        found.push((stage, "error", d.file.clone(), d.line, d.column, d.message.clone()));
    }
    if found.iter().any(|f| f.0 == "parse") {
        // What elaboration says about text that did not parse is noise.
        found.retain(|f| f.0 == "parse");
        blocked = Some("parse");
    } else if !found.is_empty() {
        blocked = Some("elaborate");
    }
    let mut modules: Vec<&str> =
        elab.hierarchy.iter().filter(|l| l.contains(" instance ")).filter_map(|l| l.rsplit(" : ").next()).collect();
    modules.sort_unstable();
    modules.dedup();

    if blocked.is_none() {
        let t = Instant::now();
        let outcome = comp.lower_netlist(&o.lower);
        times.push(("lower", t.elapsed().as_secs_f64()));
        match outcome {
            LowerOutcome::Lowered(mut nl) => {
                if o.properties {
                    compile_properties(&mut nl, o);
                }
                let _ = write!(
                    sizes,
                    ",\n  \"netlist\": {{\"values\": {}, \"inputs\": {}, \"outputs\": {}, \"states\": {}, \"memories\": {}, \"checks\": {}, \"properties\": {}}}",
                    nl.values.len(),
                    nl.inputs.len(),
                    nl.outputs.len(),
                    nl.states.len(),
                    nl.memories.len(),
                    nl.checks.len(),
                    nl.properties.len()
                );
                // The top-level inputs that clock a register, by name: what `--clock` takes.
                let mut clocks: Vec<&str> = nl
                    .states
                    .iter()
                    .filter_map(|st| st.clock)
                    .filter_map(|c| {
                        let mut v = c.signal;
                        loop {
                            match &nl.values[v.0 as usize].kind {
                                ValueKind::Input(i) => return Some(nl.inputs[*i].name.as_str()),
                                ValueKind::Op { args, .. } if args.len() == 1 => v = args[0],
                                _ => return None,
                            }
                        }
                    })
                    .collect();
                clocks.sort_unstable();
                clocks.dedup();
                let _ = write!(
                    sizes,
                    ",\n  \"clocks\": [{}]",
                    clocks.iter().map(|c| json_string(c)).collect::<Vec<_>>().join(", ")
                );
                let t = Instant::now();
                let written = svlang::btor2::write(&nl, &btor2_options(o));
                times.push(("btor2", t.elapsed().as_secs_f64()));
                match written {
                    Ok(m) => {
                        let _ = write!(
                            sizes,
                            ",\n  \"btor2\": {{\"lines\": {}, \"bytes\": {}}}",
                            m.btor2.lines().count(),
                            m.btor2.len()
                        );
                    }
                    Err(e) => {
                        found.push(("btor2", "unsupported", String::new(), 0, 0, e.to_string()));
                        blocked = Some("btor2");
                    }
                }
            }
            LowerOutcome::Unsupported(ds) | LowerOutcome::Error(ds) => {
                for d in ds {
                    let kind = if d.unsupported { "unsupported" } else { "error" };
                    found.push(("lower", kind, d.file, d.loc.line, d.loc.column, d.message));
                }
                blocked = Some("lower");
            }
        }
    }

    let mut out = String::from("{\n  \"blocked_at\": ");
    out.push_str(&blocked.map_or("null".to_string(), json_string));
    out.push_str(",\n  \"seconds\": {");
    for (i, (stage, secs)) in times.iter().enumerate() {
        let _ = write!(out, "{}\"{stage}\": {secs:.6}", if i == 0 { "" } else { ", " });
    }
    out.push('}');
    if let Some(kb) = peak_rss_kb() {
        let _ = write!(out, ",\n  \"peak_rss_kb\": {kb}");
    }
    out.push_str(&sizes);
    out.push_str(",\n  \"modules\": [");
    out.push_str(&modules.iter().map(|m| json_string(m)).collect::<Vec<_>>().join(", "));
    out.push_str("],\n  \"diagnostics\": [");
    for (i, (stage, kind, file, line, column, message)) in found.iter().enumerate() {
        let _ = write!(
            out,
            "{}\n    {{\"stage\": \"{stage}\", \"kind\": \"{kind}\", \"file\": {}, \"line\": {line}, \"column\": {column}, \"message\": {}}}",
            if i == 0 { "" } else { "," },
            json_string(file),
            json_string(message)
        );
    }
    out.push_str(if found.is_empty() { "]\n}\n" } else { "\n  ]\n}\n" });
    Ok((out, blocked.is_none()))
}

fn run(command: &str, o: &Opts) -> Result<ExitCode, Usage> {
    let failed = ExitCode::from(1);
    match command {
        "stages" => {
            let (json, passed) = stages(o)?;
            emit(o, json.as_bytes())?;
            Ok(if passed { ExitCode::SUCCESS } else { failed })
        }
        "check" => {
            let diags = compile(o)?.diagnostics();
            let errors = diags.iter().filter(|d| d.severity == Severity::Error).count();
            if o.json {
                print!("{}", diagnostics_json(&diags));
            } else {
                for d in &diags {
                    println!("{d}");
                }
                println!("{errors} error(s), {} warning(s)", diags.len() - errors);
            }
            Ok(if errors == 0 { ExitCode::SUCCESS } else { failed })
        }
        "preprocess" => {
            let comp = compile(o)?;
            let mut loc = svlang::Locator::default();
            let (mut text, mut errors) = (String::new(), 0);
            for (ix, tree) in comp.trees().iter().enumerate() {
                for d in &tree.pp_diagnostics {
                    let (f, line, column) = loc.position(ix, tree, d.origin);
                    let file = loc.files().get(f as usize).map_or("", String::as_str);
                    eprintln!("{file}:{line}:{column}: error: {}", d.message);
                    errors += 1;
                }
                for (i, token) in tree.tokens.iter().enumerate() {
                    // Line breaks and spaces as written (the text before a token also holds
                    // comments, directives and macro uses); inside an expansion, the macro's
                    // own spacing.
                    let gap = tree.trivia(i as u32);
                    if i == 0 {
                    } else if gap.contains('\n') {
                        text.push('\n');
                    } else if gap.contains(char::is_whitespace)
                        || gap.contains("/*")
                        || (token.from_macro && token.space_before)
                    {
                        text.push(' ');
                    }
                    text.push_str(&token.text);
                }
                text.push('\n');
            }
            emit(o, text.as_bytes())?;
            Ok(if errors == 0 { ExitCode::SUCCESS } else { failed })
        }
        "hier" => {
            let comp = compile(o)?;
            let (diags, elab) = comp.check();
            let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
            for d in &errors {
                eprintln!("{d}");
            }
            for line in &elab.hierarchy {
                println!("{line}");
            }
            Ok(if errors.is_empty() { ExitCode::SUCCESS } else { failed })
        }
        "cst" => {
            let comp = compile(o)?;
            for tree in comp.trees() {
                print!("{}", tree.debug_dump());
            }
            Ok(if comp.has_syntax_errors() { failed } else { ExitCode::SUCCESS })
        }
        "netlist" => {
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            emit(o, nl.dump().as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        "btor2" => {
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            match svlang::btor2::write(&nl, &btor2_options(o)) {
                Ok(m) => {
                    notes(&m.notes);
                    emit(o, m.btor2.as_bytes())?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("svlang: {e}");
                    Ok(failed)
                }
            }
        }
        "aiger" => {
            if o.output.is_none() && !o.ascii {
                return Err(Usage("binary AIGER needs -o FILE (or --ascii to print the text form)".into()));
            }
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            match svlang::aiger::lower(&nl, &btor2_options(o)) {
                Ok(m) => {
                    notes(&m.notes);
                    eprintln!("note: {}", m.aig.summary());
                    let data = if o.ascii { m.aig.to_ascii().into_bytes() } else { m.aig.to_binary() };
                    emit(o, &data)?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("svlang: {e}");
                    Ok(failed)
                }
            }
        }
        "yosys" => {
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            match svlang::yosys::write(&nl, &svlang::yosys::Options::default()) {
                Ok(m) => {
                    notes(&m.notes);
                    emit(o, m.json.as_bytes())?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("svlang: {e}");
                    Ok(failed)
                }
            }
        }
        other => Err(Usage(format!("unknown command '{other}'"))),
    }
}

fn main() -> ExitCode {
    svlang::source::exit_on_closed_pipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "version" | "--version" | "-V" => {
            println!("svlang {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    match parse(&args[1..]).and_then(|o| run(command, &o)) {
        Ok(code) => code,
        Err(Usage(msg)) => {
            eprintln!("svlang: {msg}");
            eprintln!("run 'svlang help' for usage");
            ExitCode::from(2)
        }
    }
}

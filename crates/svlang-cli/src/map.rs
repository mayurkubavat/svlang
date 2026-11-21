//! `svlang map`: the component tree of a UVM testbench, as text or JSON.
//!
//!     svlang map [--json] [--bench N] [--include-library] [--] <compile options>

use std::collections::HashSet;
use std::process::ExitCode;

use svlang_model::{Class, Design};

struct Opts {
    json: bool,
    bench: usize,
    include_library: bool,
    compile: Vec<String>,
}

fn parse(raw_args: &[String]) -> Result<Opts, Option<String>> {
    let mut o = Opts { json: false, bench: 0, include_library: false, compile: vec![] };
    let mut args = raw_args.iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => o.json = true,
            "--include-library" => o.include_library = true,
            "--bench" => {
                o.bench =
                    args.next().and_then(|n| n.parse().ok()).ok_or_else(|| Some("--bench needs a count".into()))?;
            }
            "--" => {
                o.compile.extend(args.cloned());
                break;
            }
            "-h" | "--help" => return Err(None),
            _ => o.compile.push(a.clone()),
        }
    }
    if o.compile.is_empty() {
        return Err(Some("no sources given".into()));
    }
    Ok(o)
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn tree_text(d: &Design, c: &Class, inst: &str, depth: usize, seen: &mut HashSet<u32>, out: &mut String) {
    out.push_str(&format!(
        "{}{} : {} ({}) {}:{}\n",
        "  ".repeat(depth),
        inst,
        c.name,
        c.uvm.as_str(),
        d.file_name(c.loc),
        c.loc.line
    ));
    if !seen.insert(c.id) {
        return;
    }
    for (p, t) in d.child_components(c) {
        tree_text(d, t, &p.name, depth + 1, seen, out);
    }
    seen.remove(&c.id);
}

fn tree_json(d: &Design, c: &Class, inst: &str, seen: &mut HashSet<u32>) -> String {
    let mut kids = Vec::new();
    if seen.insert(c.id) {
        for (p, t) in d.child_components(c) {
            kids.push(tree_json(d, t, &p.name, seen));
        }
        seen.remove(&c.id);
    }
    format!(
        "{{\"instance\":\"{}\",\"class\":\"{}\",\"role\":\"{}\",\"file\":\"{}\",\"line\":{},\"children\":[{}]}}",
        esc(inst),
        esc(&c.name),
        c.uvm.as_str(),
        esc(d.file_name(c.loc)),
        c.loc.line,
        kids.join(",")
    )
}

pub fn run(raw_args: &[String]) -> ExitCode {
    let o = match parse(raw_args) {
        Ok(o) => o,
        Err(None) => {
            println!("usage: svlang map [--json] [--bench N] [--include-library] [--] <compile options>");
            return ExitCode::SUCCESS;
        }
        Err(Some(e)) => {
            eprintln!("svlang map: {e}");
            eprintln!("usage: svlang map [--json] [--bench N] [--include-library] [--] <compile options>");
            return ExitCode::from(2);
        }
    };

    if o.bench > 0 {
        return bench(&o);
    }

    let d = match svlang_model::load(&o.compile) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("svlang map: {e}");
            return ExitCode::from(3);
        }
    };
    let roots: Vec<&Class> = d
        .classes()
        .iter()
        .filter(|c| c.uvm == svlang_model::UvmKind::Test && !c.is_virtual)
        .filter(|c| o.include_library || !(c.is_library || c.name.starts_with("uvm_")))
        .collect();
    let mut seen = HashSet::new();
    if o.json {
        let tests: Vec<String> = roots.iter().map(|c| tree_json(&d, c, "uvm_test_top", &mut seen)).collect();
        println!(
            "{{\"schema\":\"svlang-map/1\",\"compile_errors\":{},\"classes\":{},\"tests\":[{}]}}",
            d.compile_errors(),
            d.classes().len(),
            tests.join(",")
        );
    } else {
        let mut out = String::new();
        for c in &roots {
            tree_text(&d, c, "uvm_test_top", 0, &mut seen, &mut out);
        }
        if roots.is_empty() {
            out.push_str("no uvm_test classes found\n");
        }
        print!("{out}");
        eprintln!("svlang map: {} classes, {} compile errors", d.classes().len(), d.compile_errors());
    }
    ExitCode::SUCCESS
}

fn bench(o: &Opts) -> ExitCode {
    type Row = (svlang_model::Timings, f64, usize, usize);
    let mut rows: Vec<Row> = Vec::new();
    for _ in 0..o.bench {
        let start = std::time::Instant::now();
        let d = match svlang_model::load(&o.compile) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("svlang map: {e}");
                return ExitCode::from(3);
            }
        };
        let wall = start.elapsed().as_secs_f64() * 1e3;
        let t = d.timings();
        let members: usize = d.classes().iter().map(|c| c.properties.len() + c.methods.len()).sum();
        rows.push((t, wall, d.classes().len(), members));
    }
    let med = |f: &dyn Fn(&Row) -> f64| {
        let mut v: Vec<f64> = rows.iter().map(f).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let parse = med(&|r| r.0.parse_ms);
    let elab = med(&|r| r.0.elaborate_ms);
    let model = med(&|r| r.0.model_ms);
    let copy = med(&|r| r.0.copy_ms);
    let wall = med(&|r| r.1);
    let cpp = parse + elab + model;
    println!(
        "{{\"runs\":{},\"classes\":{},\"members\":{},\"parse_ms\":{:.1},\"elaborate_ms\":{:.1},\"model_ms\":{:.1},\
\"copy_ms\":{:.2},\"setup_ms\":{:.1},\"wall_ms\":{:.1},\"cpp_ms\":{:.1},\"copy_pct\":{:.2},\"wall_over_cpp_pct\":{:.2}}}",
        o.bench,
        rows[0].2,
        rows[0].3,
        parse,
        elab,
        model,
        copy,
        wall - cpp - copy,
        wall,
        cpp,
        copy / cpp * 100.0,
        (wall - cpp) / cpp * 100.0
    );
    ExitCode::SUCCESS
}

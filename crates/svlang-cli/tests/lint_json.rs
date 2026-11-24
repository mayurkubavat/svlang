//! `svlang lint --json FILE` writes svlang-lint's JSON report form.

#[test]
fn json_report_has_svlang_lints_fields() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-json-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("t.sv");
    std::fs::write(&src, "module m; logic [7:0] a; initial a = 255; // TODO: \"quoted\"\nendmodule\n").unwrap();
    let out = dir.join("r.json");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .args(["--preset", "style", "--json"])
        .arg(&out)
        .arg(&src)
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "only infos and warnings: exit 0, as svlang-lint");
    let text = std::fs::read_to_string(&out).unwrap();
    for key in ["\"tool\": \"svlang-lint\"", "\"compileErrors\": 0", "\"violations\": [", "\"ruleId\": \"SVL-STY-002\""]
    {
        assert!(text.contains(key), "{key} in {text}");
    }
    // The TODO comment's quotes are escaped.
    assert!(text.contains("\\\"quoted\\\""), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn presets_select_rules_as_svlang_lint() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-preset-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("t.sv");
    // SVL-STY-002 (a TODO comment) is a style rule, not a soundness one.
    std::fs::write(&src, "module m; // TODO: later\nendmodule\n").unwrap();
    let run = |extra: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .args(extra)
            .arg(&src)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).contains("SVL-STY-002")
    };
    assert!(!run(&[]), "the default selection has no style rules");
    assert!(!run(&["--preset", "default"]));
    assert!(run(&["--preset", "style"]));
    assert!(run(&["--preset", "all"]));
    assert!(!run(&["--preset", "soundness"]));
    assert!(!run(&["--preset=soundness"]));
    assert!(run(&["--preset", "soundness", "--rule", "SVL-STY-002"]), "--rule wins");
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .args(["--preset", "nope"])
        .arg(&src)
        .status()
        .unwrap();
    assert_eq!(bad.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_config_paths_rules_and_waivers() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-config-{}", std::process::id()));
    for d in [".svlang-lint", "vip", "tb"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(
        dir.join(".svlang-lint/config.json"),
        r#"{"preset": "all", "library_paths": ["vip/**"], "rules": {"SVL-STY-002": "off", "SVL-DEP-007": {"severity": "ERROR"}},
            "waivers": [{"rule": "SVL-STY-008", "path": "tb/top.sv", "line": 3, "reason": "known"}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("tb/top.sv"),
        "module top; // TODO x\nreg a; integer i;\ninitial i = 5;\ninitial i = 6;\nendmodule\n",
    )
    .unwrap();
    std::fs::write(dir.join("vip/v.sv"), "module v; reg c; endmodule\n").unwrap();
    let run = |extra: &[&str]| {
        let out = dir.join("r.json");
        std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(extra)
            .arg("--json")
            .arg(&out)
            .args(["tb/top.sv", "vip/v.sv"])
            .output()
            .unwrap();
        std::fs::read_to_string(&out).unwrap()
    };
    let with = run(&[]);
    assert!(!with.contains("SVL-STY-002"), "rule turned off: {with}");
    assert!(!with.contains("v.sv"), "library code is not reported: {with}");
    assert!(
        with.contains(
            "\"ruleId\": \"SVL-DEP-007\", \"ruleName\": \"LegacyRegWireInTestbench\", \"severity\": \"ERROR\""
        ),
        "{with}"
    );
    assert!(with.contains("\"waived\": 1"), "{with}");
    assert!(
        with.contains("\"line\": 4, \"column\": 13, \"message\": \"Unsized decimal literal '6'"),
        "not waived: {with}"
    );
    let without = run(&["--no-config", "--preset", "all"]);
    assert!(without.contains("SVL-STY-002") && without.contains("v.sv"), "{without}");
    // Without the config's preset the default rule set runs: no style rules.
    let default = run(&["--no-config"]);
    assert!(!default.contains("SVL-STY-002") && default.contains("SVL-DEP-007"), "{default}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rule_options_from_the_config() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-options-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("svlang-lint.json"),
        r#"{"rules": {"MaxLineLength": {"options": {"max_columns": 40}}, "SVL-MNT-031": {"options": {"max_args": 2}}}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("a.sv"),
        "module m;\nfunction automatic int f(int a, int b, int c); return a + b + c; endfunction // a long line here\nendmodule\n",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .arg("a.sv")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("'f' takes 3 arguments (limit 2)"), "{text}");
    assert!(text.contains("threshold of 40 characters"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn style_block_from_the_config() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-style-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("svlang-lint.json"),
        r#"{"style": {"max_line_length": 30, "allow_tabs": true, "mandatory_matching_end_label": false,
            "require_file_header": true, "file_header_pattern": "Copyright|SPDX-License-Identifier",
            "file_matches_class_name": true}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("bar.sv"),
        "class foo;\n\tint a;\n  function void f(); endfunction // padding padding\nendclass\n",
    )
    .unwrap();
    std::fs::write(dir.join("ok.sv"), "// SPDX-License-Identifier: MIT\nclass ok; endclass\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["bar.sv", "ok.sv"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let has = |id: &str, file: &str| text.lines().any(|l| l.contains(id) && l.contains(file));
    assert!(has("SVL-STY-103", "bar.sv") && text.contains("limit of 30 characters"), "{text}");
    assert!(!text.contains("SVL-STY-101"), "tabs allowed: {text}");
    assert!(!text.contains("SVL-STY-104"), "end labels not required: {text}");
    assert!(has("SVL-STY-105", "bar.sv") && !has("SVL-STY-105", "ok.sv"), "{text}");
    assert!(has("SVL-STY-106", "bar.sv") && !has("SVL-STY-106", "ok.sv"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn naming_block_from_the_config() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-naming-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("svlang-lint.json"),
        r#"{"naming": {"Class": {"enabled": true}, "Typedef": {"suffix": "_type", "severity": "ERROR"},
            "Package": {"suffix": ""}, "Coverpoint": {"pattern": "^x_.*$"}}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("t.sv"), "package my_stuff; endpackage\nclass myThing; endclass\ntypedef int count;\n")
        .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--rule", "SVL-NAM-100", "t.sv"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("Class identifier 'myThing' does not satisfy naming convention: must follow PascalCase."),
        "{text}"
    );
    assert!(text.contains("[ERROR] SVL-NAM-111") && text.contains("must end with suffix '_type'"), "{text}");
    assert!(
        text.contains("Package identifier 'my_stuff' does not satisfy naming convention: must match pattern"),
        "suffix dropped, pattern kept: {text}"
    );
    assert!(!text.contains("must end with suffix '_pkg'"), "{text}");
    assert!(!err.contains("naming.Coverpoint"), "a regex pattern is applied: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn waiver_files_regrades_and_baselines() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-baseline-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".svlang-lint")).unwrap();
    std::fs::write(
        dir.join(".svlang-lint/config.json"),
        r#"{"preset": "all", "waivers": [{"rule": "SVL-DEP-007", "severity": "error", "reason": "regrade"}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("w.json"),
        r#"{"waivers": [{"rule": "SVL-STY-008", "file": "*t.sv*", "line": 3, "reason": "known"}]}"#,
    )
    .unwrap();
    let src = dir.join("t.sv");
    std::fs::write(&src, "module m;\nreg a; integer i;\ninitial i = 5;\ninitial i = 6;\nendmodule\n").unwrap();
    let run = |extra: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(extra)
            .arg("t.sv")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let with = run(&["--waivers", "w.json"]);
    assert!(with.contains("[ERROR] SVL-DEP-007") && with.contains("[re-graded by waiver #1 (SVL-DEP-007)]"), "{with}");
    assert!(!with.contains("Unsized decimal literal '5'") && with.contains("Unsized decimal literal '6'"), "{with}");
    // A baseline of everything, then a line moves: still matched.
    run(&["--update-baseline", "b.json"]);
    let moved = std::fs::read_to_string(&src).unwrap();
    std::fs::write(&src, format!("// moved\n{moved}module n; reg z; endmodule\n")).unwrap();
    let after = run(&["--baseline", "b.json"]);
    let reported: Vec<&str> = after.lines().filter(|l| l.contains(": [")).collect();
    assert!(reported.iter().all(|l| l.contains("t.sv:7:")), "only the new module's findings: {after}");
    assert!(!reported.is_empty(), "{after}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sarif_report() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-sarif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.sv"), "module m; reg a; endmodule\n").unwrap();
    let out = dir.join("r.sarif");
    std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-config", "--sarif"])
        .arg(&out)
        .arg("t.sv")
        .output()
        .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    for key in [
        "\"version\": \"2.1.0\"",
        "\"name\": \"svlang-lint\"",
        "\"id\": \"SVL-DEP-007\"",
        "\"ruleId\": \"SVL-DEP-007\",\n          \"level\": \"note\"",
        "\"remediationMinutes\": 5",
    ] {
        assert!(text.contains(key), "{key} in {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_dashboard() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-html-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.sv"), "module m; reg a; endmodule\n").unwrap();
    let out = dir.join("r.html");
    std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-config", "--html"])
        .arg(&out)
        .arg("t.sv")
        .output()
        .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("<!DOCTYPE html>") && text.trim_end().ends_with("</html>"), "{text}");
    assert!(text.contains("<span class=\"rule-id\">SVL-DEP-007</span>"), "{text}");
    assert!(text.contains("module m; reg a; endmodule</div>"), "the source line: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inline_waiver_patterns_are_regexes() {
    // Line 2: "literal .9." does not match the message about '5', so it stays
    // (as with svlang-lint's std::regex). Line 3: a regex that matches waives.
    // Line 4: a pattern svlang lint refuses (a backreference) waives nothing and
    // is a SVL-WVR-002 error at the comment, whatever --rule selects.
    let dir = std::env::temp_dir().join(format!("svlang-lint-inline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("t.sv"),
        "module m; integer i;\ninitial i = 5; // svlang-lint:waive SVL-STY-008 \"literal .9.\"\n\
         initial i = 6; // svlang-lint:waive SVL-STY-008 \"literal '[0-9]'\"\n\
         initial i = 7; // svlang-lint:waive SVL-STY-008 \"(7)\\\\1\"\nendmodule\n",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-config", "--no-cache", "--rule", "SVL-STY-008", "t.sv"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let sty: Vec<&str> = text.lines().filter(|l| l.contains("] SVL-STY-008 (")).collect();
    assert_eq!(sty.len(), 2, "{text}");
    assert!(sty[0].contains("t.sv:2:") && sty[1].contains("t.sv:4:"), "{text}");
    let wv: Vec<&str> = text.lines().filter(|l| l.contains("SVL-WVR-002")).collect();
    assert_eq!(wv.len(), 1, "{text}");
    assert!(wv[0].contains("t.sv:4:1:") && wv[0].contains("invalid message regex '(7)\\1'"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn waiver_file_with_a_bad_pattern_stops_the_run() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-waiver-re-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.sv"), "module m; integer i; initial i = 5; endmodule\n").unwrap();
    std::fs::write(dir.join("w.json"), r#"[{"rule": "SVL-STY-008", "message": "(?=x)", "reason": "r"}]"#).unwrap();
    std::fs::write(dir.join("ok.json"), r#"[{"rule": "SVL-STY-008", "message": "literal '[5]'", "reason": "r"}]"#)
        .unwrap();
    let run = |w: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(["--no-config", "--no-cache", "--rule", "SVL-STY-008", "--waivers", w, "t.sv"])
            .output()
            .unwrap()
    };
    let bad = run("w.json");
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("invalid message regex '(?=x)'"));
    let ok = run("ok.json");
    assert!(!String::from_utf8_lossy(&ok.stdout).contains("SVL-STY-008"), "{}", String::from_utf8_lossy(&ok.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fix_rewrites_and_dry_run_shows_the_diff() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-fix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("a.sv");
    std::fs::write(&src, "module m;   \n  logic a;\t\nendmodule  \n").unwrap();
    let run = |extra: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(["--no-config", "--fix-rule", "SVL-STY-012"])
            .args(extra)
            .arg("a.sv")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let dry = run(&["--fix-dry-run"]);
    assert!(dry.starts_with("--- a/a.sv\n+++ b/a.sv\n@@ -1,3 +1,3 @@\n-module m;   \n+module m;\n"), "{dry}");
    assert!(dry.contains("[svlang-lint] Would apply 3 fix-it(s) in 1 file(s)."), "{dry}");
    assert_eq!(
        std::fs::read_to_string(&src).unwrap(),
        "module m;   \n  logic a;\t\nendmodule  \n",
        "dry run writes nothing"
    );
    let fixed = run(&["--fix"]);
    assert!(fixed.contains("[svlang-lint] Applied 3 fix-it(s) in 1 file(s)."), "{fixed}");
    assert_eq!(std::fs::read_to_string(&src).unwrap(), "module m;\n  logic a;\nendmodule\n");
    assert!(!fixed.contains("SVL-STY-012"), "the next pass reports on the fixed text: {fixed}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fix_skips_files_with_semantic_errors() {
    // A `solve before` on a non-random property: slang rejects the file and
    // svlang-lint applies no fix in it; the native front end now agrees.
    let dir = std::env::temp_dir().join(format!("svlang-lint-fix-cmp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = "class c;   \n  rand int w; int plain;\n  constraint k_c { solve plain before w; }\nendclass\n";
    std::fs::write(dir.join("a.sv"), text).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-config", "--no-cache", "--fix-rule", "SVL-STY-012", "--fix", "a.sv"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("1 in files with compile errors not applied"), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("a.sv")).unwrap(), text, "the file is left as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inputs_that_cannot_be_read_exit_2() {
    // As svlang-lint: the run cannot start (docs/guide/svlang-lint.md, Exit status).
    let dir = std::env::temp_dir().join(format!("svlang-lint-exit2-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("adir")).unwrap();
    std::fs::write(dir.join("ok.sv"), "module m; endmodule\n").unwrap();
    std::fs::write(dir.join("nested.f"), "-f gone.f\n").unwrap();
    std::fs::write(dir.join("self.f"), "-f self.f\n").unwrap();
    let code = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(["--no-config", "--no-cache"])
            .args(args)
            .output()
            .unwrap()
            .status
            .code()
    };
    for args in [
        &["missing.sv"][..],
        &["ok.sv", "adir"],
        &["ok.sv", "-f", "nested.f"],
        &["ok.sv", "-f", "self.f"],
        &["ok.sv", "-v", "missing_lib.v"],
        &["ok.sv", "-y", "no_dir"],
        &["--no-such-option", "ok.sv"],
    ] {
        assert_eq!(code(args), Some(2), "{args:?}");
    }
    assert_eq!(code(&["ok.sv", "-I", "no_dir"]), Some(0), "a missing include directory is not an error");
    assert_eq!(code(&["ok.sv", "+acc", "-l", "run.log"]), Some(0), "simulator options are ignored");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn redundant_boolean_fix_keeps_x_and_z_behaviour() {
    // SVL-MNT-046: `b == 1'b0` -> `!b` everywhere (both X for X and Z);
    // `b == 1'b1` -> `b` only in a condition or on a 2-state `b`.
    let dir = std::env::temp_dir().join(format!("svlang-lint-fix-rdb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = "module m(input logic a, input bit b, output logic y, output logic z, output bit q);\n\
               always_comb begin\n\
               if (a == 1'b1) y = 1'b0;\n\
               z = (a == 1'b0);\n\
               z = (a == 1'b1);\n\
               q = (b == 1'b1);\n\
               end\n\
               endmodule\n";
    let run = |extra: &[&str]| {
        std::fs::write(dir.join("a.sv"), src).unwrap();
        std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args([
                "--no-config",
                "--no-cache",
                "--preset",
                "all",
                "--experimental",
                "--fix",
                "--fix-rule",
                "SVL-MNT-046",
            ])
            .args(extra)
            .arg("a.sv")
            .output()
            .unwrap();
        std::fs::read_to_string(dir.join("a.sv")).unwrap()
    };
    let safe = run(&[]);
    assert!(safe.contains("if (a) y = 1'b0;"), "{safe}");
    assert!(safe.contains("z = (!a);"), "{safe}");
    assert!(safe.contains("z = (a == 1'b1);"), "a 4-state value outside a condition is left: {safe}");
    assert!(safe.contains("q = (b);"), "{safe}");
    let all = run(&["--unsafe-fixes"]);
    assert!(all.contains("z = (a);"), "{all}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn banner_fix_makes_78_columns() {
    // SVL-STY-005 / SVL-STY-006: a ragged separator comment becomes 78 characters.
    let dir = std::env::temp_dir().join(format!("svlang-lint-fix-banner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.sv"),
        "class c;\n  //----------\n  function void f(); endfunction\n  //======\nendclass\n",
    )
    .unwrap();
    std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-config", "--no-cache", "--fix", "--fix-rule", "SVL-STY-005,SVL-STY-006", "a.sv"])
        .output()
        .unwrap();
    let text = std::fs::read_to_string(dir.join("a.sv")).unwrap();
    assert!(text.contains(&format!("  //{}\n", "-".repeat(76))), "{text}");
    assert!(text.contains(&format!("  //{}\n", "=".repeat(76))), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exit_status_follows_errors_only() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-exit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("w.sv"), "module m; // TODO: x\nendmodule\n").unwrap();
    // A syntax error is an ERROR finding (CMP-ERR-001).
    std::fs::write(dir.join("e.sv"), "module m; logic a endmodule\n").unwrap();
    let code = |f: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(["--no-config", f])
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(code("w.sv"), Some(0));
    assert_eq!(code("e.sv"), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rule the project config configures runs although the default rule set
/// leaves its profile out; `--preset` on the command line narrows the run
/// again, and `enabled: false` is not a request for the rule.
#[test]
fn a_configured_rule_runs_outside_the_default_set() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-configured-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".svlang-lint")).unwrap();
    // SVL-STY-002 (a TODO comment) and SVL-STY-012 (trailing whitespace) are style rules.
    std::fs::write(dir.join("t.sv"), "module m; // TODO: later  \nendmodule\n").unwrap();
    let run = |config: &str, extra: &[&str]| {
        std::fs::write(dir.join(".svlang-lint/config.json"), config).unwrap();
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lint")
            .current_dir(&dir)
            .args(["--no-cache"])
            .args(extra)
            .arg("t.sv")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
    };
    let plain = run("{}", &[]);
    assert!(!plain.contains("SVL-STY-002") && !plain.contains("SVL-STY-012"), "{plain}");
    let configured = run(r#"{"rules": {"SVL-STY-002": {"severity": "ERROR"}, "TrailingWhitespace": false}}"#, &[]);
    assert!(configured.contains("[ERROR] SVL-STY-002"), "{configured}");
    assert!(!configured.contains("SVL-STY-012"), "turned off is not asked for: {configured}");
    let narrowed = run(r#"{"rules": {"SVL-STY-002": {"severity": "ERROR"}}}"#, &["--preset", "soundness"]);
    assert!(!narrowed.contains("SVL-STY-002"), "{narrowed}");
    // A style block asks for the style engine (SVL-STY-100, reporting as SVL-STY-102).
    let block = run(r#"{"style": {"allow_trailing_whitespace": false}}"#, &[]);
    assert!(block.contains("SVL-STY-102"), "{block}");
    // The project's preset stands in for --preset.
    let preset = run(r#"{"preset": "style"}"#, &[]);
    assert!(preset.contains("SVL-STY-002") && preset.contains("SVL-STY-012"), "{preset}");
    let bad = run(r#"{"preset": "nope"}"#, &[]);
    assert!(bad.contains("preset: unknown preset 'nope'"), "{bad}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verissimo_native_fixes_preserve_semantics() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-vfix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // 1. SVL-STY-014, SVL-STY-015, SVL-STY-016, SVL-CON-013 safe fixes.
    std::fs::write(
        dir.join("m.sv"),
        "`define ADD4(x) x + 4\nmodule m;\n  logic a; logic b;\n      string s;\n  initial begin\n    s <= \"hi\";\n  end\nendmodule\n",
    )
    .unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args([
            "--no-config",
            "--no-cache",
            "--fix",
            "--fix-rule",
            "SVL-STY-014,SVL-STY-015,SVL-STY-016,SVL-CON-013",
            "m.sv",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    let fixed = std::fs::read_to_string(dir.join("m.sv")).unwrap();
    assert!(fixed.contains("`define ADD4(x) (x) + 4"), "{fixed}");
    assert!(fixed.contains("\n  logic a;\n  logic b;\n  string s;\n"), "{fixed}");
    assert!(fixed.contains("s = \"hi\";"), "{fixed}");

    // 2. UVM-DEP-005 safe fix (`starting_phase` -> `get_starting_phase()`).
    std::fs::write(
        dir.join("seq.sv"),
        "`include \"uvm_mock.svh\"\nclass s_seq extends uvm_sequence;\n  `uvm_object_utils(s_seq)\n  function new(string name = \"s_seq\"); super.new(name); endfunction\n  virtual task body();\n    if (starting_phase != null) begin\n      starting_phase.raise_objection(this, \"r\");\n      starting_phase.drop_objection(this, \"r\");\n    end\n  endtask\nendclass\n",
    )
    .unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args([
            "--no-config",
            "--no-cache",
            "-I",
            repo.join("testdata/common").to_str().unwrap(),
            "--fix",
            "--fix-rule",
            "UVM-DEP-005",
            "seq.sv",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    let fixed_seq = std::fs::read_to_string(dir.join("seq.sv")).unwrap();
    assert!(fixed_seq.contains("if (get_starting_phase() != null) begin"), "{fixed_seq}");
    assert!(fixed_seq.contains("get_starting_phase().raise_objection(this, \"r\");"), "{fixed_seq}");
    assert!(fixed_seq.contains("get_starting_phase().drop_objection(this, \"r\");"), "{fixed_seq}");

    // 3. SVL-MNT-001 + SVL-STY-019 safe fixes (`ref` -> `const ref`, tabular port alignment).
    std::fs::write(
        dir.join("align_and_ref.sv"),
        "`timescale 1ns/1ps\nmodule aligned_ports (\n    input  logic       clk,\n    input  logic       rst_n,\n    input  logic [7:0] din,\n    output logic [7:0]   dout\n);\n    assign dout = din ^ {8{clk & rst_n}};\nendmodule\nclass ref_helper;\n    function int peek_first(ref int arr[]);\n        return arr.size();\n    endfunction\nendclass\n",
    )
    .unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args([
            "--no-config",
            "--no-cache",
            "--preset",
            "all",
            "--fix",
            "--fix-rule",
            "SVL-MNT-001,SVL-STY-019",
            "align_and_ref.sv",
        ])
        .output()
        .unwrap();
    assert!(o.status.code().is_some_and(|c| c <= 1), "{}", String::from_utf8_lossy(&o.stdout));
    let fixed_ar = std::fs::read_to_string(dir.join("align_and_ref.sv")).unwrap();
    assert!(fixed_ar.contains("    output logic [7:0] dout\n"), "{fixed_ar}");
    assert!(fixed_ar.contains("function int peek_first(const ref int arr[]);"), "{fixed_ar}");

    // 4. SVL-PRF-001, SVL-CON-008, and UVM-SEQ-018 `--unsafe-fixes` rewrites.
    std::fs::write(
        dir.join("unsafe_fixes.sv"),
        "`include \"uvm_mock.svh\"\nclass pkt_helper extends uvm_sequence #(uvm_sequence_item);\n  `uvm_object_utils(pkt_helper)\n  function new(string name = \"pkt_helper\"); super.new(name); endfunction\n  function int count_bytes(byte payload[]);\n    return payload.size();\n  endfunction\n  task body();\n    starting_phase.raise_objection(this, \"go\");\n    for (int i = 0; i < 2; i++) begin\n      fork\n        #1 $display(\"%0d\", i);\n      join_none\n    end\n    starting_phase.drop_objection(this, \"done\");\n  endtask\nendclass\n",
    )
    .unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args([
            "--no-config",
            "--no-cache",
            "-I",
            repo.join("testdata/common").to_str().unwrap(),
            "--fix",
            "--unsafe-fixes",
            "--fix-rule",
            "SVL-PRF-001,SVL-CON-008,UVM-SEQ-018",
            "unsafe_fixes.sv",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    let fixed_u = std::fs::read_to_string(dir.join("unsafe_fixes.sv")).unwrap();
    assert!(fixed_u.contains("function int count_bytes(const ref byte payload[]);"), "{fixed_u}");
    assert!(fixed_u.contains("automatic int __i = i;"), "{fixed_u}");
    assert!(fixed_u.contains("#1 $display(\"%0d\", __i);"), "{fixed_u}");
    assert!(fixed_u.contains("if (starting_phase != null) starting_phase.raise_objection"), "{fixed_u}");
    assert!(fixed_u.contains("if (starting_phase != null) starting_phase.drop_objection"), "{fixed_u}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefix_wildcard_selectors_work_across_cli_config_and_inline_waivers() {
    let dir = std::env::temp_dir().join(format!("svlang-lint-wildcards-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".svlang-lint")).unwrap();
    let src = dir.join("top.sv");
    std::fs::write(&src, "module top; // TODO: check\n  reg r;\nendmodule\n").unwrap();

    // 1. CLI `--rule 'SVL-STY-*'` selects SVL-STY-002 without running SVL-DEP-007.
    let cli_sty = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .args(["lint", "--no-config", "--no-cache", "--rule", "SVL-STY-*"])
        .arg(&src)
        .output()
        .unwrap();
    let out_sty = String::from_utf8_lossy(&cli_sty.stdout);
    assert!(out_sty.contains("SVL-STY-002"), "{out_sty}");
    assert!(!out_sty.contains("SVL-DEP-007"), "{out_sty}");

    // 2. Comma-separated CLI `--rule 'SVL-STY-*,SVL-DEP-*'` selects both prefixes.
    let cli_both = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .args(["lint", "--no-config", "--no-cache", "--rule", "SVL-STY-*,SVL-DEP-*"])
        .arg(&src)
        .output()
        .unwrap();
    let out_both = String::from_utf8_lossy(&cli_both.stdout);
    assert!(out_both.contains("SVL-STY-002") && out_both.contains("SVL-DEP-007"), "{out_both}");

    // 3. Project config `"rules": {"SVL-STY-*": "off"}` + `"waivers": [{"rule": "SVL-DEP-*", ...}]`.
    std::fs::write(
        dir.join(".svlang-lint/config.json"),
        r#"{"preset": "all", "rules": {"SVL-STY-*": "off"}, "waivers": [{"rule": "SVL-DEP-*", "path": "top.sv", "reason": "legacy reg"}]}"#,
    )
    .unwrap();
    let report = dir.join("r.json");
    let cfg_run = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .arg("lint")
        .current_dir(&dir)
        .args(["--no-cache", "--json"])
        .arg(&report)
        .arg("top.sv")
        .status()
        .unwrap();
    assert_eq!(cfg_run.code(), Some(0));
    let rjson = std::fs::read_to_string(&report).unwrap();
    assert!(!rjson.contains("SVL-STY-002"), "turned off by SVL-STY-*: {rjson}");
    assert!(!rjson.contains("SVL-DEP-007"), "waived by SVL-DEP-*: {rjson}");
    assert!(rjson.contains("\"waived\": 1"), "{rjson}");

    // 4. Inline and scoped block waivers with prefix wildcards (`SVL-STY-*`, `SVL-*`).
    let waived_src = dir.join("waived.sv");
    std::fs::write(
        &waived_src,
        "module waived;\n  // TODO: inline // svlang-lint:waive SVL-STY-*\n  // svlang-lint:off SVL-DEP-*\n  reg r;\n  // svlang-lint:on SVL-DEP-*\nendmodule\n",
    )
    .unwrap();
    let inline_run = std::process::Command::new(env!("CARGO_BIN_EXE_svlang"))
        .args(["lint", "--no-config", "--no-cache", "--preset", "all", "--warn-unused-waivers"])
        .arg(&waived_src)
        .output()
        .unwrap();
    let out_inline = String::from_utf8_lossy(&inline_run.stdout);
    assert!(!out_inline.contains("SVL-STY-002"), "{out_inline}");
    assert!(!out_inline.contains("SVL-DEP-007"), "{out_inline}");
    assert!(!out_inline.contains("SVL-WVR-002"), "wildcard waivers matched cleanly: {out_inline}");

    let _ = std::fs::remove_dir_all(&dir);
}

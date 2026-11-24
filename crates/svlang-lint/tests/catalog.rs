//! The catalog the rules declare: complete, consistent, and the one
//! docs/rules/catalog.json publishes.

use svlang_lint::catalog;

/// Normative 2-tier 3-3-3 (`DOM-GRP-NNN`) prefix-to-category allocation table.
const ALLOWED_PREFIXES: &[(&str, &str)] = &[
    ("CMP-ERR", "Compiler"),
    ("RTL-SYN", "RTL"),
    ("RTL-FSM", "RTL"),
    ("RTL-CDC", "RTL"),
    ("RTL-DAT", "RTL"),
    ("SVL-COR", "Correctness"),
    ("SVL-TYP", "Types"),
    ("SVL-OOP", "OOP"),
    ("SVL-CON", "Concurrency"),
    ("SVL-RND", "Randomization"),
    ("SVL-COV", "Coverage"),
    ("SVL-PRF", "Performance"),
    ("SVL-DED", "DeadCode"),
    ("SVL-MNT", "Maintainability"),
    ("SVL-WVR", "Maintainability"),
    ("SVL-PRT", "Portability"),
    ("SVL-DPI", "Portability"),
    ("SVL-SEC", "Security"),
    ("SVL-DEP", "Deprecated"),
    ("SVL-STY", "Style"),
    ("SVL-NAM", "Naming"),
    ("SVA-PRP", "SVA"),
    ("SVA-FPV", "SVA"),
    ("UVM-FAC", "UVM"),
    ("UVM-PHS", "UVM"),
    ("UVM-OBJ", "UVM"),
    ("UVM-CFG", "UVM"),
    ("UVM-SEQ", "UVM"),
    ("UVM-TLM", "UVM"),
    ("UVM-CMP", "UVM"),
    ("UVM-RAL", "UVM"),
    ("UVM-DEP", "Deprecated"),
    ("UVM-PRT", "Portability"),
    ("UVM-NAM", "Naming"),
];

#[test]
fn every_rule_is_fully_declared_and_follows_3_3_3_taxonomy() {
    let all = catalog::entries();
    assert_eq!(all.len(), 475, "expected 475 catalog entries (446 rules + 29 sub-rules)");
    let guide: Vec<&str> = catalog::guidelines().iter().map(|g| g.id).collect();
    for pair in all.windows(2) {
        assert_ne!(pair[0].id, pair[1].id, "two entries share an id");
    }
    for e in &all {
        let parts: Vec<&str> = e.id.split('-').collect();
        assert_eq!(parts.len(), 3, "{}: rule ID must be DOM-GRP-NNN", e.id);
        assert!(
            matches!(parts[0], "CMP" | "RTL" | "SVL" | "SVA" | "UVM") && parts[0].len() == 3,
            "{}: invalid 3-letter domain {}",
            e.id,
            parts[0]
        );
        assert!(
            parts[1].len() == 3 && parts[1].bytes().all(|b| b.is_ascii_uppercase()),
            "{}: invalid 3-letter group {}",
            e.id,
            parts[1]
        );
        assert!(
            parts[2].len() == 3 && parts[2].bytes().all(|b| b.is_ascii_digit()) && parts[2] != "000",
            "{}: invalid 3-digit number {}",
            e.id,
            parts[2]
        );
        let prefix = format!("{}-{}", parts[0], parts[1]);
        let Some(&(_, expected_cat)) = ALLOWED_PREFIXES.iter().find(|&&(p, _)| p == prefix) else {
            panic!("{}: unallocated prefix {} (register in ALLOWED_PREFIXES & CONTRIBUTING.md)", e.id, prefix);
        };
        assert_eq!(
            e.category, expected_cat,
            "{}: prefix {} requires category {}, got {}",
            e.id, prefix, expected_cat, e.category
        );
        assert!(catalog::CATEGORIES.contains(&e.category), "{}: unknown category {}", e.id, e.category);
        assert!(e.profiles != 0, "{}: no profile, so no preset selects it", e.id);
        assert!(e.about.len() > 20 && e.about.ends_with('.'), "{}: description is not a sentence", e.id);
        assert!(!e.name.is_empty() && e.name.chars().all(|c| c.is_ascii_alphanumeric()), "{}: name {}", e.id, e.name);
        for g in e.guidelines {
            assert!(guide.contains(g), "{}: guideline {g} is not in docs/methodology", e.id);
        }
    }
}

#[test]
fn prefix_wildcard_selector_matches_domains_groups_and_names() {
    assert!(svlang_lint::rule_selector_matches("RTL-*", "RTL-FSM-002", "FsmUnreachableState"));
    assert!(!svlang_lint::rule_selector_matches("RTL-*", "SVL-COR-004", "SelfComparison"));
    assert!(svlang_lint::rule_selector_matches("SVL-CON-*", "SVL-CON-008", "ForkLoopVariableCapture"));
    assert!(!svlang_lint::rule_selector_matches("SVL-CON-*", "SVL-COR-008", "IdenticalBranches"));
    assert!(svlang_lint::rule_selector_matches("UVM-RAL-001", "UVM-RAL-001", "DirectMirrorSetWithoutWrite"));
    assert!(svlang_lint::rule_selector_matches("FsmUnreachableState", "RTL-FSM-002", "FsmUnreachableState"));
    assert!(svlang_lint::rule_selector_matches("*", "SVA-FPV-001", "AssumeOnOutputOrState"));
}

#[test]
fn published_catalog_is_current() {
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/rules/catalog.json");
    let published = std::fs::read_to_string(file).unwrap();
    assert!(published == catalog::rules_json(), "docs/rules/catalog.json is stale: run tools/codegen/gen_rule_docs.py");
}

#[test]
fn presets_follow_the_declared_profiles() {
    let all = svlang_lint::rules();
    let style = svlang_lint::rule_selection(Some("style"), false).unwrap();
    let default = svlang_lint::rule_selection(None, false).unwrap();
    let with_experimental = svlang_lint::rule_selection(Some("all"), true).unwrap();
    for r in &all {
        assert_eq!(style(r.id), !r.experimental && r.profiles & svlang_lint::profile::STYLE != 0, "{}", r.id);
        assert_eq!(default(r.id), !r.experimental && r.profiles & svlang_lint::DEFAULT_PROFILES != 0, "{}", r.id);
        assert!(with_experimental(r.id), "{}", r.id);
    }
    // Not a built-in rule: a Python rule always runs.
    assert!(style("MY-RULE-001"));
}

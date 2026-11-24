//! Inline waivers, as svlang-lint's WaiverEngine reads them:
//!
//! - on the finding's own line: `svlang-lint:waive RULE`, `svlang-lint:disable`,
//!   `svlang-lint-disable`, `lint:waive`, `lint:off`, `svlang-lint: waive`;
//! - on the line above: `svlang-lint-disable-next-line RULE`,
//!   `svlang-lint:waive-next-line`, `svlang-lint:disable-next-line`.
//!
//! `*` waives any rule. A quoted pattern after the rule narrows the waiver to
//! findings whose message it matches, searched for as `std::regex_search`
//! does ([`crate::pattern`]). A pattern that does not compile, or a quote left
//! open, waives nothing and is reported as a SVL-WVR-002 error at the comment,
//! as svlang-lint reports it.

use std::collections::{HashMap, HashSet};

use crate::Finding;

const SAME_LINE: [&str; 8] = [
    "svlang-lint:waive",
    "svlang-lint:disable",
    "svlang-lint-disable",
    "lint:waive",
    "lint:off",
    "svlang-lint: waive",
    "verissimo_lint: waive_line",
    "verissimo_lint:waive_line",
];
const NEXT_LINE: [&str; 6] = [
    "svlang-lint-disable-next-line",
    "svlang-lint:waive-next-line",
    "svlang-lint:disable-next-line",
    "svlang-lint: waive",
    "verissimo_lint: waive_line",
    "verissimo_lint:waive_line",
];
const BLOCK_OFF: [&str; 6] = [
    "svlang-lint:off",
    "svlang-lint: off",
    "svlang-lint-off",
    "verissimo_lint: waive_start",
    "verissimo_lint:waive_start",
    "lint:waive-start",
];
const BLOCK_ON: [&str; 8] = [
    "svlang-lint:on",
    "svlang-lint: on",
    "svlang-lint-on",
    "verissimo_lint: waive_stop",
    "verissimo_lint:waive_stop",
    "lint:waive-stop",
    "lint:on",
    "lint: on",
];

/// The quoted pattern after a keyword, if any; Err holds what an
/// unterminated one had read (a backslash keeps the character after it).
fn message_pattern(rest: &str) -> Result<Option<String>, String> {
    let Some(q) = rest.find('"') else { return Ok(None) };
    let mut out = String::new();
    let mut it = rest[q + 1..].chars();
    while let Some(ch) = it.next() {
        match ch {
            '\\' => out.extend(it.next()),
            '"' => return Ok(Some(out)),
            _ => out.push(ch),
        }
    }
    Err(out)
}

/// The message of the SVL-WVR-002 error for a waiver comment whose pattern
/// is unusable (svlang-lint's wording).
pub(crate) const BAD_PATTERN: &str = "invalid message regex '";

/// Inline patterns, compiled once each: Err holds why one does not compile.
type Compiled = HashMap<String, Result<crate::pattern::Pattern, String>>;

/// A waiver comment's unusable pattern: (comment line, pattern, why).
type Bad = Vec<(u32, String, String)>;

fn waives(text: &str, f: &Finding, at: u32, same_line: bool, compiled: &mut Compiled, bad: &mut Bad) -> bool {
    let keywords: &[&str] = if same_line { &SAME_LINE } else { &NEXT_LINE };
    for kw in keywords {
        let mut from = 0;
        while let Some(p) = text[from..].find(kw) {
            let pos = from + p;
            from = pos + 1;
            let rest = &text[pos + kw.len()..];
            // `svlang-lint:waive` is a prefix of `svlang-lint:waive-next-line`.
            if rest.starts_with('-') || rest.starts_with(':') {
                continue;
            }
            let head = rest.split('"').next().unwrap_or(rest);
            if !rule_ids(head).any(|r| crate::rule_selector_matches(r, f.rule, "")) {
                continue;
            }
            match message_pattern(rest) {
                Err(open) => {
                    bad.push((at, open, "unterminated message regex".into()));
                    return false;
                }
                Ok(Some(p)) if !p.is_empty() => {
                    let re = compiled.entry(p.clone()).or_insert_with(|| crate::pattern::Pattern::new(&p, false));
                    match re {
                        Ok(re) if re.is_match_anywhere(&f.message) => return true,
                        Ok(_) => continue,
                        Err(e) => {
                            bad.push((at, p, e.clone()));
                            continue;
                        }
                    }
                }
                _ => return true,
            }
        }
    }
    false
}

/// What inline comments waived so far, for the waiver report: the comment
/// lines used (file, line) and the waived findings per rule.
#[derive(Clone, Debug, Default)]
pub(crate) struct Used {
    pub(crate) lines: HashSet<(String, u32)>,
    pub(crate) by_rule: HashMap<String, usize>,
    /// The waived findings, with their file: a waiver file entry that also
    /// matches one counts as used, as in svlang-lint.
    pub(crate) waived: Vec<(String, u32, &'static str, String)>,
}

/// Session-scoped inline waiver tracking state.
#[derive(Clone, Debug, Default)]
pub(crate) struct WaiverState {
    pub(crate) inline_waived: usize,
    pub(crate) used: Used,
}

#[derive(Clone, Debug)]
struct BlockSpan {
    start_line: u32,
    end_line: u32,
    rules: Vec<String>,
    rest: String,
}

fn comment_tail<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    let pos = line.find(kw)?;
    let prefix = &line[..pos];
    let after_delim =
        prefix.rfind("//").map(|i| &prefix[i + 2..]).or_else(|| prefix.rfind("/*").map(|i| &prefix[i + 2..]))?;
    if !after_delim.chars().all(|c| c.is_whitespace() || c == '*') {
        return None;
    }
    let rest = &line[pos + kw.len()..];
    if rest.starts_with('-') || rest.starts_with(':') {
        return None;
    }
    Some(rest)
}

fn parse_blocks(lines: &[String]) -> (Vec<BlockSpan>, Vec<(u32, String)>) {
    let has_lint_on =
        lines.iter().any(|l| comment_tail(l, "lint:on").is_some() || comment_tail(l, "lint: on").is_some());
    // Active open blocks: (start_line, rules, rest_text, strict_unclosed)
    let mut open: Vec<(u32, Vec<String>, String, bool)> = Vec::new();
    let mut spans = Vec::new();
    let total = lines.len() as u32;
    for (idx, line) in lines.iter().enumerate() {
        let lno = idx as u32 + 1;
        if let Some(rest) = BLOCK_ON.iter().find_map(|kw| comment_tail(line, kw)) {
            let close_ids: Vec<&str> = rule_ids(rest).collect();
            let close_all = close_ids.is_empty() || close_ids.contains(&"*");
            let mut still_open = Vec::new();
            for (sline, mut rlist, srest, strict) in open.drain(..) {
                if close_all {
                    spans.push(BlockSpan { start_line: sline, end_line: lno, rules: rlist, rest: srest });
                } else {
                    let (closed, rem): (Vec<String>, Vec<String>) =
                        rlist.drain(..).partition(|r| r != "*" && close_ids.contains(&r.as_str()));
                    if !closed.is_empty() {
                        spans.push(BlockSpan { start_line: sline, end_line: lno, rules: closed, rest: srest.clone() });
                    }
                    if !rem.is_empty() {
                        still_open.push((sline, rem, srest, strict));
                    }
                }
            }
            open = still_open;
            continue;
        }
        if let Some(rest) = BLOCK_OFF.iter().find_map(|kw| comment_tail(line, kw)) {
            let mut ids: Vec<String> = rule_ids(rest).map(str::to_string).collect();
            if ids.is_empty() {
                ids.push("*".to_string());
            }
            open.push((lno, ids, rest.to_string(), true));
            continue;
        }
        if has_lint_on {
            if let Some(rest) = comment_tail(line, "lint:off").or_else(|| comment_tail(line, "lint: off")) {
                let mut ids: Vec<String> = rule_ids(rest).map(str::to_string).collect();
                if ids.is_empty() {
                    ids.push("*".to_string());
                }
                open.push((lno, ids, rest.to_string(), false));
            }
        }
    }
    let mut unclosed = Vec::new();
    for (sline, rlist, srest, strict) in open {
        if strict {
            unclosed.push((sline, rlist.join(", ")));
            spans.push(BlockSpan { start_line: sline, end_line: total.max(sline), rules: rlist, rest: srest });
        }
    }
    (spans, unclosed)
}

/// Returns `(line_number, rule_list)` for every strict block waiver (`svlang-lint:off`,
/// `verissimo_lint: waive_start`, etc.) in `src` that is never closed before EOF.
pub(crate) fn find_unclosed_blocks(src: &str) -> Vec<(u32, String)> {
    let lines: Vec<String> = src.lines().map(str::to_string).collect();
    parse_blocks(&lines).1
}

fn waives_in_blocks(
    spans: &[BlockSpan],
    f: &Finding,
    line: u32,
    compiled: &mut Compiled,
    bad: &mut Bad,
) -> Option<u32> {
    if f.rule == "SVL-WVR-001" {
        return None;
    }
    for sp in spans {
        if line < sp.start_line || line > sp.end_line {
            continue;
        }
        if !sp.rules.iter().any(|r| crate::rule_selector_matches(r, f.rule, "")) {
            continue;
        }
        match message_pattern(&sp.rest) {
            Err(open) => {
                bad.push((sp.start_line, open, "unterminated message regex".into()));
                return None;
            }
            Ok(Some(p)) if !p.is_empty() => {
                let re = compiled.entry(p.clone()).or_insert_with(|| crate::pattern::Pattern::new(&p, false));
                match re {
                    Ok(re) if re.is_match_anywhere(&f.message) => return Some(sp.start_line),
                    Ok(_) => continue,
                    Err(e) => {
                        bad.push((sp.start_line, p, e.clone()));
                        continue;
                    }
                }
            }
            _ => return Some(sp.start_line),
        }
    }
    None
}

type FileWaiverData = (Vec<String>, Vec<BlockSpan>);

/// Drops findings an inline comment waives, recording into `state`; returns how many.
pub(crate) fn apply_into(
    findings: &mut Vec<Finding>,
    files: &[String],
    state: &std::sync::Mutex<WaiverState>,
) -> usize {
    let before = findings.len();
    let mut used = Used::default();
    let mut texts: HashMap<String, Option<FileWaiverData>> = HashMap::new();
    let mut compiled = Compiled::new();
    // Unusable patterns, by file: each comment and pattern reported once.
    let mut bad_at: Vec<(String, u32, String, String)> = Vec::new();
    findings.retain(|f| {
        let (path, line, _) = f.position_in(files);
        if path.is_empty() || line == 0 {
            return true;
        }
        let cached = texts.entry(path.to_string()).or_insert_with(|| {
            svlang::syntax::overlay::read(std::path::Path::new(path)).ok().map(|t| {
                let ls: Vec<String> = t.lines().map(str::to_string).collect();
                let (spans, _) = parse_blocks(&ls);
                (ls, spans)
            })
        });
        let Some((lines, spans)) = cached else { return true };
        let at = |l: u32| lines.get(l as usize - 1).map(String::as_str).unwrap_or("");
        let mut bad = Bad::new();
        let by = if waives(at(line), f, line, true, &mut compiled, &mut bad) {
            Some(line)
        } else if line > 1 && waives(at(line - 1), f, line - 1, false, &mut compiled, &mut bad) {
            Some(line - 1)
        } else {
            waives_in_blocks(spans, f, line, &mut compiled, &mut bad)
        };
        for (l, p, e) in bad {
            if !bad_at.iter().any(|(bf, bl, bp, _)| bf == path && *bl == l && *bp == p) {
                bad_at.push((path.to_string(), l, p, e));
            }
        }
        if let Some(l) = by {
            used.lines.insert((path.to_string(), l));
            *used.by_rule.entry(f.rule.to_string()).or_default() += 1;
            used.waived.push((path.to_string(), line, f.rule, f.message.clone()));
        }
        by.is_none()
    });
    let waived = before - findings.len();
    for (path, line, pattern, why) in bad_at {
        let Some(file) = files.iter().position(|f| *f == path) else { continue };
        findings.push(Finding {
            rule: "SVL-WVR-002",
            name: "UnusedWaiver",
            severity: crate::Severity::Error,
            message: format!("{BAD_PATTERN}{pattern}': {why}"),
            loc: svlang_model::Loc { file: file as u32, line, column: 1, expansion: None },
            related: Vec::new(),
            fixes: Vec::new(),
        });
    }
    if let Ok(mut s) = state.lock() {
        s.inline_waived += waived;
        s.used.lines.extend(used.lines);
        s.used.waived.extend(used.waived);
        for (r, n) in used.by_rule {
            *s.used.by_rule.entry(r).or_default() += n;
        }
    }
    waived
}

/// Drops findings an inline comment waives in the active session; returns how many.
pub(crate) fn apply(findings: &mut Vec<Finding>, files: &[String]) -> usize {
    let state = crate::session::with(|s| s.waiver_state.clone());
    apply_into(findings, files, &state)
}

/// The SVL-WVR-002 errors [`apply`] added for unusable waiver patterns: kept
/// out of the config, waivers and `--changed-since`, as svlang-lint adds them
/// after those.
pub(crate) fn is_bad_pattern(f: &Finding) -> bool {
    f.rule == "SVL-WVR-002" && f.message.starts_with(BAD_PATTERN)
}

/// A waiver comment that waived nothing: (file, line, comment text).
pub type Unused = (String, u32, String);

/// The waiver comments in `files` against an explicit `used` line set.
pub(crate) fn unused_comments_with(
    files: &[String],
    active: &HashSet<&str>,
    used: &HashSet<(String, u32)>,
) -> (usize, Vec<Unused>) {
    const KEYWORDS: [&str; 16] = [
        "svlang-lint-disable-next-line",
        "svlang-lint:waive-next-line",
        "svlang-lint:disable-next-line",
        "svlang-lint:waive",
        "svlang-lint:disable",
        "svlang-lint-disable",
        "svlang-lint:off",
        "svlang-lint: off",
        "svlang-lint-off",
        "verissimo_lint: waive_start",
        "verissimo_lint:waive_start",
        "verissimo_lint: waive_line",
        "verissimo_lint:waive_line",
        "lint:waive-start",
        "lint:waive",
        "lint:off",
    ];
    let mut count = 0;
    let mut unused = Vec::new();
    for file in files {
        let Ok(text) = svlang::syntax::overlay::read(std::path::Path::new(file)) else { continue };
        for (i, l) in text.lines().enumerate() {
            let Some(at) = KEYWORDS.iter().filter_map(|k| l.find(k)).min() else { continue };
            let comment = l[..at].rfind("//");
            if comment.is_none() && !l[..at].contains("/*") {
                continue;
            }
            count += 1;
            let line = i as u32 + 1;
            if used.contains(&(file.clone(), line)) {
                continue;
            }
            let head = l[at..].split('"').next().unwrap_or(&l[at..]);
            if rule_ids(head).any(|r| r == "*" || active.iter().any(|a| crate::rule_selector_matches(r, a, ""))) {
                unused.push((file.clone(), line, l[comment.unwrap_or(at)..].to_string()));
            }
        }
    }
    (count, unused)
}

/// The waiver comments in `files` in the active session.
pub(crate) fn unused_comments(files: &[String], active: &HashSet<&str>) -> (usize, Vec<Unused>) {
    let used = crate::session::with(|s| s.waiver_state.lock().ok().map(|st| st.used.lines.clone()).unwrap_or_default());
    unused_comments_with(files, active, &used)
}

/// The rule IDs (`SVL-STY-001`, `UVM-NAM-101.x`) and `*` in `text`, as
/// svlang-lint's `[A-Z][A-Z0-9]*-[A-Z][A-Z0-9]*-[0-9]+[A-Za-z0-9.]*|\*`.
fn rule_ids(text: &str) -> impl Iterator<Item = &str> {
    let b = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            if b[i] == b'*' {
                i += 1;
                return Some(&text[i - 1..i]);
            }
            if let Some(n) = id_at(&b[i..]) {
                let s = &text[i..i + n];
                i += n;
                return Some(s);
            }
            i += 1;
        }
        None
    })
}

/// The length of a rule ID or prefix wildcard (`RTL-*`, `SVL-STY-*`) at the start of `b`.
fn id_at(b: &[u8]) -> Option<usize> {
    let up = |c: u8| c.is_ascii_uppercase();
    let upnum = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
    let mut i = 0;
    for seg in 0..2 {
        if !b.get(i).copied().is_some_and(up) {
            return None;
        }
        i += 1;
        while b.get(i).copied().is_some_and(upnum) {
            i += 1;
        }
        if b.get(i) != Some(&b'-') {
            return None;
        }
        i += 1;
        if seg == 0 && b.get(i) == Some(&b'*') {
            return Some(i + 1);
        }
    }
    if b.get(i) == Some(&b'*') {
        return Some(i + 1);
    }
    let digits = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == digits {
        return None;
    }
    while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'.') {
        i += 1;
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rule_ids_as_svlang_lint_does() {
        let ids: Vec<&str> =
            rule_ids(" SVL-STY-001 -- x, RTL-*, SVL-NAM-*, UVM-NAM-101.a and * not sv-sty-1 or A-B-").collect();
        assert_eq!(ids, ["SVL-STY-001", "RTL-*", "SVL-NAM-*", "UVM-NAM-101.a", "*"]);
    }

    #[test]
    fn parses_closed_and_unclosed_block_waivers() {
        let lines: Vec<String> = [
            "// svlang-lint:off SVL-STY-001",
            "int a;",
            "// svlang-lint:on SVL-STY-001",
            "// verissimo_lint: waive_start SVL-DED-001",
            "int b;",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let (spans, unclosed) = parse_blocks(&lines);
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].start_line, spans[0].end_line), (1, 3));
        assert_eq!(unclosed, vec![(4, "SVL-DED-001".to_string())]);
    }
}

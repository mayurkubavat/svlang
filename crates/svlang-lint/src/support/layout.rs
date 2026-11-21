//! Token-trivia layout and preprocessor event index (`TreeLayout`), built once
//! per tree via [`crate::shared::tree_layout`] to power formatting, macro
//! hygiene, and include-hygiene rules without re-lexing source buffers.

#![allow(dead_code)]

use svlang::syntax::{IncludeEdge, MacroDef, Origin, Tree};

/// Per-line layout metadata (0-indexed in `FileLayout::lines` for 1-based line `l`).
#[derive(Clone, Debug)]
pub(crate) struct LineLayout {
    pub(crate) start: u32,
    pub(crate) end: u32,
    /// Visual indentation width in columns (space = 1, tab = 4).
    pub(crate) indent_cols: u32,
    pub(crate) has_leading_tab: bool,
    pub(crate) blank_or_comment: bool,
    pub(crate) in_define_or_skipped: bool,
    /// First non-macro token originating on this line, if any.
    pub(crate) first_token: Option<u32>,
}

/// Layout index for one source buffer in [`Tree::files`].
#[derive(Clone, Debug)]
pub(crate) struct FileLayout {
    pub(crate) lines: Vec<LineLayout>,
}

impl FileLayout {
    /// 1-based line lookup.
    pub(crate) fn line(&self, line_1based: u32) -> Option<&LineLayout> {
        line_1based.checked_sub(1).and_then(|i| self.lines.get(i as usize))
    }
}

/// Cached layout and preprocessor index for a [`Tree`].
#[derive(Clone, Debug)]
pub(crate) struct TreeLayout {
    pub(crate) files: Vec<FileLayout>,
}

impl TreeLayout {
    pub(crate) fn build(tree: &Tree) -> Self {
        let mut files = Vec::with_capacity(tree.sources.len());
        for (fidx, src) in tree.sources.iter().enumerate() {
            let f_u32 = fidx as u32;
            let bytes = src.as_bytes();
            let mut raw_lines: Vec<(u32, u32)> = Vec::new();
            let mut start = 0usize;
            for i in 0..=bytes.len() {
                if i == bytes.len() || bytes[i] == b'\n' {
                    let mut end = i;
                    if end > start && bytes[end - 1] == b'\r' {
                        end -= 1;
                    }
                    raw_lines.push((start as u32, end as u32));
                    start = i + 1;
                }
            }

            let mut in_block_comment = false;
            let mut in_multiline_define = false;
            let mut lines: Vec<LineLayout> = Vec::with_capacity(raw_lines.len());
            for &(ls, le) in &raw_lines {
                let slice = &src[ls as usize..le as usize];
                let mut indent_cols = 0u32;
                let mut has_leading_tab = false;
                let mut ws_bytes = 0usize;
                for b in slice.bytes() {
                    match b {
                        b' ' => {
                            indent_cols += 1;
                            ws_bytes += 1;
                        }
                        b'\t' => {
                            indent_cols += 4 - (indent_cols % 4);
                            has_leading_tab = true;
                            ws_bytes += 1;
                        }
                        _ => break,
                    }
                }
                let trimmed = &slice[ws_bytes..];
                let starts_define = !in_block_comment && trimmed.starts_with("`define");
                let in_def = in_multiline_define || starts_define;

                // Track block comments and blank/comment-only status on this line.
                let mut blank_or_comment = true;
                let mut k = ws_bytes;
                let sb = slice.as_bytes();
                while k < sb.len() {
                    if in_block_comment {
                        if k + 1 < sb.len() && sb[k] == b'*' && sb[k + 1] == b'/' {
                            in_block_comment = false;
                            k += 2;
                        } else {
                            k += 1;
                        }
                    } else if k + 1 < sb.len() && sb[k] == b'/' && sb[k + 1] == b'/' {
                        break;
                    } else if k + 1 < sb.len() && sb[k] == b'/' && sb[k + 1] == b'*' {
                        in_block_comment = true;
                        k += 2;
                    } else if sb[k].is_ascii_whitespace() {
                        k += 1;
                    } else {
                        blank_or_comment = false;
                        k += 1;
                    }
                }

                let continued = slice.trim_end_matches([' ', '\t']).ends_with('\\');
                in_multiline_define = in_def && continued;

                let skipped = tree.skipped.iter().any(|&(sf, sa, se)| sf == f_u32 && ls >= sa && le <= se);

                lines.push(LineLayout {
                    start: ls,
                    end: le,
                    indent_cols,
                    has_leading_tab,
                    blank_or_comment,
                    in_define_or_skipped: in_def || skipped,
                    first_token: None,
                });
            }

            files.push(FileLayout { lines });
        }

        // Map first non-macro source token per line.
        for (tidx, tok) in tree.tokens.iter().enumerate() {
            if tok.from_macro {
                continue;
            }
            let Origin { file, offset } = tok.origin;
            let Some(fl) = files.get_mut(file as usize) else {
                continue;
            };
            let starts = tree.line_starts(file as usize);
            let line_idx = starts.partition_point(|&s| s <= offset).saturating_sub(1);
            if let Some(ll) = fl.lines.get_mut(line_idx) {
                if ll.first_token.is_none() {
                    ll.first_token = Some(tidx as u32);
                }
            }
        }

        TreeLayout { files }
    }

    pub(crate) fn file(&self, file: u32) -> Option<&FileLayout> {
        self.files.get(file as usize)
    }

    /// Returns `(file, 1-based line, 1-based column)` of a non-macro token.
    pub(crate) fn source_token_pos(&self, tree: &Tree, tok: u32) -> Option<(u32, u32, u32)> {
        let t = tree.tokens.get(tok as usize)?;
        if t.from_macro {
            return None;
        }
        let f = t.origin.file;
        let starts = tree.line_starts(f as usize);
        let line_1 = starts.partition_point(|&s| s <= t.origin.offset) as u32;
        let line_start = starts.get(line_1.saturating_sub(1) as usize).copied().unwrap_or(0);
        Some((f, line_1, t.origin.offset.saturating_sub(line_start) + 1))
    }

    /// Top-level macro definitions in main files of `tree`.
    pub(crate) fn main_macro_defs<'t>(&self, tree: &'t Tree) -> Vec<&'t MacroDef> {
        tree.macro_defs.iter().filter(|d| tree.mains.contains(&d.origin.file)).collect()
    }

    /// Top-level `` `include `` directives in main files of `tree`.
    #[allow(dead_code)]
    pub(crate) fn main_includes<'t>(&self, tree: &'t Tree) -> Vec<&'t IncludeEdge> {
        tree.includes.iter().filter(|e| tree.mains.contains(&e.origin.file)).collect()
    }
}

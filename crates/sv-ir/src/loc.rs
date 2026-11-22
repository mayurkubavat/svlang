//! Source positions: [`Loc`], and the [`Locator`] that turns token positions
//! of parsed trees into them.

use sv_syntax::Tree;

/// A source position. Lines and columns start at 1; a default `Loc` (file 0,
/// line 0) means "unknown".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loc {
    /// Index into `sv_semantic::Design::files`; 0 is "no file".
    pub file: u32,
    /// 1-based line.
    pub line: u32,
    /// 1-based byte column.
    pub column: u32,
    /// For code from a macro: the file and line of the outermost expansion
    /// (with column 0).
    pub expansion: Option<(u32, u32)>,
}

/// Turns token positions of parsed trees into [`Loc`]s, numbering files as
/// they are first seen (index 0 is "no file", as in slang's model). A
/// locator seeded with a design's files numbers them as that design does.
pub struct Locator {
    files: Vec<String>,
    file_ids: crate::fx::FxMap<(usize, u32), u32>,
}

impl Default for Locator {
    fn default() -> Self {
        Locator::new(vec![String::new()])
    }
}

impl Locator {
    /// A locator with `files` as its file table (start it with `[""]` so 0
    /// stays "no file").
    pub fn new(files: Vec<String>) -> Self {
        Locator { files, file_ids: Default::default() }
    }

    /// File names by index, the table [`Loc::file`] indexes.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// The file table, for `sv_semantic::Design::from_parts`.
    pub fn into_files(self) -> Vec<String> {
        self.files
    }

    /// Location of token `tok` of `tree` (index `tree_ix` among the trees
    /// this locator sees). Code from a macro is placed where its text was
    /// written, as slang's model does, and also remembers the expansion
    /// site, where findings on it are reported.
    pub fn loc(&mut self, tree_ix: usize, tree: &Tree, tok: u32) -> Loc {
        let Some(token) = tree.tokens.get(tok as usize) else {
            return Loc::default();
        };
        let (written, origin, from_macro) = (token.written, token.origin, token.from_macro);
        let (file, line, column) = self.position(tree_ix, tree, written);
        let expansion = from_macro.then(|| {
            let (f, l, _) = self.position(tree_ix, tree, origin);
            (f, l)
        });
        Loc { file, line, column, expansion }
    }

    /// Design file index, line and column of a preprocessor origin.
    pub fn position(&mut self, tree_ix: usize, t: &Tree, o: sv_syntax::Origin) -> (u32, u32, u32) {
        let key = (tree_ix, o.file);
        let file = match self.file_ids.get(&key) {
            Some(&f) => f,
            None => {
                let name = t.files[o.file as usize].to_string_lossy().into_owned();
                let f = match self.files.iter().position(|n| *n == name) {
                    Some(i) => i as u32,
                    None => {
                        self.files.push(name);
                        (self.files.len() - 1) as u32
                    }
                };
                self.file_ids.insert(key, f);
                f
            }
        };
        let src = &t.sources[o.file as usize];
        let off = (o.offset as usize).min(src.len()) as u32;
        let starts = t.line_starts(o.file as usize);
        // The last line start at or before the offset.
        let line = starts.partition_point(|&s| s <= off);
        (file, line as u32, off - starts[line - 1] + 1)
    }
}

//! A Btor2 model as parsed lines: just enough structure to bit-blast it.

use std::collections::BTreeMap;

/// A sort: a bit-vector of a given width or an array between two sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    BitVec(u32),
    Array { index: u32, element: u32 },
}

macro_rules! ops {
    ($($v:ident => $s:literal),* $(,)?) => {
        /// Every Btor2 operator except `sort`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Op { $($v),* }
        impl Op {
            pub fn name(self) -> &'static str { match self { $(Op::$v => $s),* } }
            pub fn from_name(s: &str) -> Option<Op> { match s { $($s => Some(Op::$v),)* _ => None } }
        }
    };
}

ops! {
    Input => "input", State => "state", Init => "init", Next => "next",
    Bad => "bad", Constraint => "constraint", Fair => "fair", Justice => "justice", Output => "output",
    Const => "const", Constd => "constd", Consth => "consth", Zero => "zero", One => "one", Ones => "ones",
    Sext => "sext", Uext => "uext", Slice => "slice",
    Not => "not", Inc => "inc", Dec => "dec", Neg => "neg", Redand => "redand", Redor => "redor", Redxor => "redxor",
    Iff => "iff", Implies => "implies", Eq => "eq", Neq => "neq",
    Sgt => "sgt", Ugt => "ugt", Sgte => "sgte", Ugte => "ugte", Slt => "slt", Ult => "ult", Slte => "slte", Ulte => "ulte",
    And => "and", Nand => "nand", Nor => "nor", Or => "or", Xnor => "xnor", Xor => "xor",
    Rol => "rol", Ror => "ror", Sll => "sll", Sra => "sra", Srl => "srl",
    Add => "add", Mul => "mul", Sdiv => "sdiv", Udiv => "udiv", Smod => "smod", Srem => "srem", Urem => "urem", Sub => "sub",
    Saddo => "saddo", Uaddo => "uaddo", Sdivo => "sdivo", Udivo => "udivo", Smulo => "smulo", Umulo => "umulo", Ssubo => "ssubo", Usubo => "usubo",
    Concat => "concat", Read => "read", Ite => "ite", Write => "write",
}

/// How many fields of each kind a Btor2 operator takes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpShape {
    pub has_sort: bool,
    pub args: usize,
    pub nums: usize,
    pub variadic: bool,
}

/// One non-sort Btor2 line. Arguments are signed: `-n` negates node `n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: u32,
    pub op: Op,
    pub sort: Option<u32>,
    pub args: Vec<i64>,
    /// Literal parameters: constant digits, slice bounds, extension width.
    pub nums: Vec<String>,
    pub symbol: Option<String>,
}

/// A model entry in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Sort { id: u32, sort: Sort, symbol: Option<String> },
    Line(Line),
}

/// A word-level transition system in Btor2 form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    entries: Vec<Entry>,
    index: BTreeMap<u32, usize>,
}

impl Model {
    pub fn last_id(&self) -> Option<u32> {
        self.index.keys().next_back().copied()
    }

    pub fn sort(&self, id: u32) -> Option<Sort> {
        match self.entries.get(*self.index.get(&id)?)? {
            Entry::Sort { sort, .. } => Some(*sort),
            Entry::Line(_) => None,
        }
    }

    pub fn line(&self, id: u32) -> Option<&Line> {
        match self.entries.get(*self.index.get(&id)?)? {
            Entry::Line(l) => Some(l),
            Entry::Sort { .. } => None,
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Line(l) => Some(l),
            _ => None,
        })
    }

    /// Lines with the given operator, in file order.
    pub fn with_op(&self, op: Op) -> impl Iterator<Item = &Line> {
        self.lines().filter(move |l| l.op == op)
    }

    pub(crate) fn push_sort(&mut self, id: u32, sort: Sort, symbol: Option<String>) {
        self.index.insert(id, self.entries.len());
        self.entries.push(Entry::Sort { id, sort, symbol });
    }

    pub(crate) fn push_line(&mut self, line: Line) {
        self.index.insert(line.id, self.entries.len());
        self.entries.push(Entry::Line(line));
    }

    /// Width in bits of a bit-vector sort, or `None` for arrays.
    pub fn width(&self, sort: u32) -> Option<u32> {
        match self.sort(sort)? {
            Sort::BitVec(w) => Some(w),
            Sort::Array { .. } => None,
        }
    }
}

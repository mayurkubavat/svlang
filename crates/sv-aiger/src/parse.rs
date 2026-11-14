//! Btor2 reader.
//!
//! Format reference: Niemetz, Preiner, Wolf, Biere, "Btor2, BtorMC and
//! Boolector 3.0", CAV 2018. Every line is `<id> <op> ...`; a negative
//! argument `-n` means the bitwise negation of node `n`.

use crate::model::{Line, Model, Op, OpShape, Sort};

/// Error raised while reading a Btor2 file. `line` is 1-based.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "btor2 line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError { line, msg: msg.into() })
}

/// Parse Btor2 text into a [`Model`]. Checks that ids are unique and
/// increasing, every reference points at an earlier line, every sort
/// argument names a sort, and every node argument names a node.
pub fn parse(text: &str) -> Result<Model, ParseError> {
    let mut model = Model::default();
    for (idx, raw) in text.lines().enumerate() {
        let lno = idx + 1;
        let content = raw.split(';').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let toks: Vec<&str> = content.split_whitespace().collect();
        let id: u32 = match toks[0].parse() {
            Ok(v) if v > 0 => v,
            _ => return err(lno, format!("bad id '{}'", toks[0])),
        };
        if let Some(last) = model.last_id() {
            if id <= last {
                return err(lno, format!("id {id} is not greater than previous id {last}"));
            }
        }
        let Some(opname) = toks.get(1) else {
            return err(lno, "missing operator");
        };
        let mut rest = &toks[2..];

        if *opname == "sort" {
            let sort = match rest {
                ["bitvec", w, tail @ ..] => {
                    let w: u32 = w
                        .parse()
                        .ok()
                        .filter(|w| *w > 0)
                        .ok_or_else(|| ParseError { line: lno, msg: format!("bad bit-vector width '{w}'") })?;
                    rest = tail;
                    Sort::BitVec(w)
                }
                ["array", i, e, tail @ ..] => {
                    let i = sort_ref(&model, i, lno)?;
                    let e = sort_ref(&model, e, lno)?;
                    rest = tail;
                    Sort::Array { index: i, element: e }
                }
                _ => return err(lno, "expected 'sort bitvec <w>' or 'sort array <i> <e>'"),
            };
            let symbol = rest.first().map(|s| s.to_string());
            model.push_sort(id, sort, symbol);
            continue;
        }

        let op = Op::from_name(opname)
            .ok_or_else(|| ParseError { line: lno, msg: format!("unknown operator '{opname}'") })?;
        let shape = op.shape();
        let mut line = Line { id, op, sort: None, args: Vec::new(), nums: Vec::new(), symbol: None };

        if shape.has_sort {
            let Some((s, tail)) = rest.split_first() else {
                return err(lno, format!("'{opname}' needs a sort"));
            };
            line.sort = Some(sort_ref(&model, s, lno)?);
            rest = tail;
        }

        let n_args = if shape.variadic {
            let Some((n, tail)) = rest.split_first() else {
                return err(lno, format!("'{opname}' needs an argument count"));
            };
            rest = tail;
            n.parse::<usize>().map_err(|_| ParseError { line: lno, msg: format!("bad count '{n}'") })?
        } else {
            shape.args
        };
        if rest.len() < n_args {
            return err(lno, format!("'{opname}' needs {n_args} argument(s)"));
        }
        for a in &rest[..n_args] {
            line.args.push(node_ref(&model, a, lno)?);
        }
        rest = &rest[n_args..];

        if rest.len() < shape.nums {
            return err(lno, format!("'{opname}' needs {} numeric parameter(s)", shape.nums));
        }
        for n in &rest[..shape.nums] {
            check_literal(op, n, lno)?;
            line.nums.push(n.to_string());
        }
        rest = &rest[shape.nums..];
        line.symbol = rest.first().map(|s| s.to_string());
        if rest.len() > 1 {
            return err(lno, format!("unexpected trailing tokens after symbol: {:?}", &rest[1..]));
        }
        model.push_line(line);
    }
    Ok(model)
}

fn sort_ref(model: &Model, tok: &str, lno: usize) -> Result<u32, ParseError> {
    let id: u32 = tok.parse().map_err(|_| ParseError { line: lno, msg: format!("bad sort id '{tok}'") })?;
    if model.sort(id).is_none() {
        return err(lno, format!("{id} is not a declared sort"));
    }
    Ok(id)
}

fn node_ref(model: &Model, tok: &str, lno: usize) -> Result<i64, ParseError> {
    let r: i64 = tok.parse().map_err(|_| ParseError { line: lno, msg: format!("bad node reference '{tok}'") })?;
    // A reference past u32 would wrap onto some earlier node.
    let id = u32::try_from(r.unsigned_abs()).unwrap_or(0);
    if r == 0 || id == 0 || model.line(id).is_none() {
        return err(lno, format!("{r} does not refer to an earlier node"));
    }
    Ok(r)
}

fn check_literal(op: Op, tok: &str, lno: usize) -> Result<(), ParseError> {
    let ok = match op {
        Op::Const => !tok.is_empty() && tok.bytes().all(|b| b == b'0' || b == b'1'),
        Op::Constd => {
            let t = tok.strip_prefix('-').unwrap_or(tok);
            !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
        }
        Op::Consth => !tok.is_empty() && tok.bytes().all(|b| b.is_ascii_hexdigit()),
        _ => tok.parse::<u32>().is_ok(),
    };
    if ok {
        Ok(())
    } else {
        err(lno, format!("bad literal '{tok}' for '{}'", op.name()))
    }
}

impl Op {
    pub(crate) fn shape(self) -> OpShape {
        use Op::*;
        let s = |has_sort, args, nums| OpShape { has_sort, args, nums, variadic: false };
        match self {
            Input | State | Zero | One | Ones => s(true, 0, 0),
            Const | Constd | Consth => s(true, 0, 1),
            Init | Next => s(true, 2, 0),
            Bad | Constraint | Fair | Output => s(false, 1, 0),
            Justice => OpShape { has_sort: false, args: 0, nums: 0, variadic: true },
            Slice => s(true, 1, 2),
            Uext | Sext => s(true, 1, 1),
            Not | Inc | Dec | Neg | Redand | Redor | Redxor => s(true, 1, 0),
            Ite | Write => s(true, 3, 0),
            _ => s(true, 2, 0),
        }
    }
}

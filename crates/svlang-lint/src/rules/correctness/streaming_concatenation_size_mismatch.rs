//! SVL-COR-025 (StreamingConcatenationSizeMismatch): Flags streaming
//! concatenation operators ({<< { ... }} / {>> { ... }}) omitting an explicit
//! slice size (advisory, IEEE 1800-2023 §11.4.14.2).

use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::Child;
use svlang::syntax::SyntaxKind::*;

pub(crate) fn stream_slice(c: &Cst, s: &mut Sink) {
    for n in c.nodes(ConcatExpr) {
        let Some(op) = c.tok(n, "<<").or_else(|| c.tok(n, ">>")) else { continue };
        // A slice size may be either a node (expression / DataType) or a built-in type token between `<<`/`>>` and `{`.
        let mut after_op = false;
        let mut sliced = false;
        for k in c.kids(n) {
            match *k {
                Child::Token(t) if t == op => after_op = true,
                Child::Token(t) if after_op && c.text_of(t) == "{" => break,
                Child::Token(t) if after_op && !c.text_of(t).trim().is_empty() => sliced = true,
                Child::Node(_) if after_op => sliced = true,
                _ => {}
            }
        }
        if sliced {
            continue;
        }
        let o = c.text_of(op);
        let msg = if o == "<<" {
            "Right-to-left streaming operator '<<' omits an explicit slice size ('{<< { ... }}'). \
             Per IEEE 1800 § 11.4.14.2, omitting slice_size defaults to 1-bit slicing (bit-reversal); \
             if byte- or word-reversal was intended, specify an explicit slice width (e.g. '{<< 8 { ... }}' or '{<< byte { ... }}')."
                .to_string()
        } else {
            "Left-to-right streaming operator '>>' omits an explicit slice size ('{>> { ... }}'). \
             While slice_size does not reorder bits on '>>' (IEEE 1800 § 11.4.14.2), specifying an explicit element unit (e.g. '{>> byte { ... }}') clarifies stream chunking."
                .to_string()
        };
        s.add("SVL-COR-025", "StreamingConcatenationSizeMismatch", Severity::Info, msg, c.loc(op));
    }
}

//! SVA-PRP-007 (ImplicationInCoverProperty): `cover property` whose top-level
//! property uses implication (`|->`, `|=>`) instead of sequence concatenation (`##`).

use svlang::syntax::SyntaxKind::*;

use crate::support::class_items::user_code;
use crate::support::slang_ast::subtree;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn implication_in_cover_property(c: &Cst, out: &mut Sink) {
    // Also map named `property p; ... |-> ... endproperty` in this tree so `cover property (p)` is caught too!
    let mut impl_props = std::collections::HashSet::new();
    for pd in c.nodes(PropertyDecl) {
        let pname = crate::support::slang_ast::decl_name(c, pd);
        if !pname.is_empty()
            && subtree(c, pd)
                .into_iter()
                .any(|s| c.kind(s) == SequenceExpr && (c.tok(s, "|->").is_some() || c.tok(s, "|=>").is_some()))
        {
            impl_props.insert(pname);
        }
    }

    for ca in c.nodes(ConcurrentAssert) {
        if !user_code(c, ca) {
            continue;
        }
        let is_cover = c.toks(ca).any(|t| c.text_of(t) == "cover");
        if !is_cover {
            continue;
        }
        let inline_imp = subtree(c, ca)
            .into_iter()
            .find(|&s| c.kind(s) == SequenceExpr && (c.tok(s, "|->").is_some() || c.tok(s, "|=>").is_some()));
        if let Some(seq) = inline_imp {
            let op_tok = c.tok(seq, "|->").or_else(|| c.tok(seq, "|=>")).or_else(|| c.first_tok(ca));
            if let Some(t) = op_tok {
                out.add(
                    "SVA-PRP-007",
                    "ImplicationInCoverProperty",
                    Severity::Warning,
                    "Cover property uses implication ('|->' or '|=>'), which matches vacuously on every cycle where the antecedent is false (IEEE 1800 § 16.14.3). Use sequence concatenation ('##0' or '##1') in cover properties so only genuine stimulus hits are counted.".into(),
                    c.loc(t),
                );
            }
            continue;
        }
        // Check named property reference `cover property (prop_name);`
        for ne in subtree(c, ca) {
            if c.kind(ne) == NameExpr {
                let id = c.tree.bare_text(ne);
                if impl_props.contains(&id) {
                    if let Some(t) = c.first_tok(ne) {
                        out.add(
                            "SVA-PRP-007",
                            "ImplicationInCoverProperty",
                            Severity::Warning,
                            format!(
                                "Cover property instantiates property '{id}' which uses implication ('|->' or '|=>') and matches vacuously when its antecedent is false; cover a sequence ('##0' / '##1') instead."
                            ),
                            c.loc(t),
                        );
                        break;
                    }
                }
            }
        }
    }
}

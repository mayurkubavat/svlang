//! SEC-LOG-001 (PlaintextKeyInLogs): Detects plaintext logging of sensitive
//! cryptographic assets (keys, seeds, secrets) via $sformatf/$display.

use crate::support::ast::{call_site, source_text, string_value, subtree, unparen, Ast};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang_model::Design;

fn plaintext_key(c: &Cst, a: &Ast, s: &mut Sink) {
    for n in subtree(c, c.tree.root) {
        let Some(call) = call_site(c, n) else { continue };
        let name = call.name.strip_prefix('$').unwrap_or(&call.name);
        if !matches!(name, "sformatf" | "display" | "write" | "writeh") || call.args.len() < 2 {
            continue;
        }
        let Some(Some(fmt)) = call.args.first().copied() else { continue };
        let Some(fmt) = string_value(c, unparen(c, fmt)) else { continue };
        if !fmt.contains('%') {
            continue;
        }
        for arg in call.args.iter().skip(1).copied().flatten() {
            let text = source_text(c, arg).trim().to_string();
            if !has_sensitive_asset_name(&text) {
                continue;
            }
            // The text keeps the parentheses; the location is inside them.
            let Some(at) = c.first_tok(unparen(c, arg)) else { continue };
            s.add(
                "SEC-LOG-001",
                "PlaintextKeyInLogs",
                Severity::Warning,
                format!(
                    "Potential plaintext logging of sensitive cryptographic asset '{text}' via ${name}(). Printing keys, seeds, or root-of-trust secrets in simulation \
                     transcripts violates hardware security standards (ISO/SAE 21434 / Common Criteria). \
                     Mask or omit sensitive secrets from regression logs."
                ),
                c.loc(at),
            );
            a.again(s, c, n);
        }
    }
}

/// Checks whether `expr` names a cryptographic key, seed, token, or secret
/// (excluding string literals and plain `.key` config/dictionary member lookups
/// unless qualified with a security prefix like `aes_key`, `secret_key`, etc.).
fn has_sensitive_asset_name(expr: &str) -> bool {
    let trimmed = expr.trim_matches(['(', ')', ' ', '\t', '\r', '\n']);
    if trimmed.starts_with('"') {
        return false;
    }
    let low = trimmed.to_lowercase();
    if ["secret", "seed", "token", "priv_"].iter().any(|w| low.contains(w)) {
        return true;
    }
    if !low.contains("key") {
        return false;
    }
    // `obj.key` on a struct/config entry is a dictionary/config key unless the
    // identifier itself is a qualified key (`aes_key`, `root_key`, `key_0`, ...).
    let last_seg = low.rsplit(['.', ':']).next().unwrap_or(&low);
    let last_ident = last_seg.split(['[', '(']).next().unwrap_or(last_seg).trim();
    !((low.contains('.') || low.contains("::")) && last_ident == "key")
}

pub(crate) fn ast_plaintext_key(d: &Design, csts: &[Cst], s: &mut Sink) {
    for c in csts {
        plaintext_key(c, &Ast::new(d, c), s);
    }
}

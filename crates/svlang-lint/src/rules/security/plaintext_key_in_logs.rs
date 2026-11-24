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

/// Checks whether `expr` names a cryptographic key, secret, password, or auth token
/// (excluding string literals, plain simulation `seed`, lexer `token`s, and generic
/// dictionary/config `key` identifiers such as `key_applied[fi.key]`).
fn has_sensitive_asset_name(expr: &str) -> bool {
    let trimmed = expr.trim_matches(['(', ')', ' ', '\t', '\r', '\n']);
    if trimmed.starts_with('"') {
        return false;
    }
    // Extract the root identifier (before `[` or `(`) of the final member/scoped segment.
    let before_idx = trimmed.split(['[', '(']).next().unwrap_or(trimmed);
    let leaf = before_idx.rsplit(['.', ':']).next().unwrap_or(before_idx).trim().to_ascii_lowercase();
    if leaf.is_empty() {
        return false;
    }
    if ["secret", "passwd", "password", "passphrase", "priv_"].iter().any(|w| leaf.contains(w)) {
        return true;
    }
    const CRYPTO_QUALIFIERS: &[&str] = &[
        "aes", "des", "rsa", "ecc", "ecdsa", "sha", "hmac", "kdf", "otp", "fuses", "fuse", "root", "master", "private",
        "priv", "enc", "dec", "cipher", "crypto", "auth", "api", "bearer", "session", "access", "tls", "ssl",
    ];
    let words: Vec<&str> = leaf.split('_').filter(|s| !s.is_empty()).collect();
    let has_qual = words.iter().any(|w| CRYPTO_QUALIFIERS.contains(w));
    if words.iter().any(|&w| matches!(w, "key" | "keys" | "token" | "tokens" | "seed")) {
        return has_qual;
    }
    false
}

pub(crate) fn ast_plaintext_key(d: &Design, csts: &[Cst], s: &mut Sink) {
    for c in csts {
        plaintext_key(c, &Ast::new(d, c), s);
    }
}

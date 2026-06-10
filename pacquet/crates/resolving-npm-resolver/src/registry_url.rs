//! Build the `<registry>/<encoded-pkg>` URL for a metadata fetch.
//!
//! Ports upstream's
//! [`toUri`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts).
//! Scoped names are routed as a single path segment by
//! percent-encoding the `/` (and other non-path-safe characters)
//! between the `@scope` prefix and the package's bare name —
//! otherwise `https://registry/@scope/pkg` would parse as two
//! segments and a registry that doesn't tolerate the un-encoded
//! form (or a CDN in front of the registry that re-canonicalizes
//! paths) would 404.
//!
//! Mirrors JS's `encodeURIComponent` for the characters npm package
//! names can carry. The grammar at
//! [the npm package-name spec](https://github.com/npm/validate-npm-package-name#naming-rules)
//! allows `a-z 0-9 _ . - ~` plus the leading `@scope/`; the leading
//! `@` is preserved (matching upstream's
//! `@${encodeURIComponent(pkgName.slice(1))}` shape), and every
//! other character that `encodeURIComponent` would touch is
//! percent-encoded.

use std::fmt::Write as _;

/// Compose the metadata-fetch URL: `<registry-with-trailing-slash><encoded-name>`.
#[must_use]
pub fn to_registry_url(registry: &str, pkg_name: &str) -> String {
    let registry =
        if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    let encoded = encode_pkg_name_path(pkg_name);
    format!("{registry}{encoded}")
}

/// `encodeURIComponent` clone for the characters npm package names
/// can carry. For a scoped name the leading `@` is preserved and
/// the rest of the name is percent-encoded — matching upstream's
/// `@${encodeURIComponent(pkgName.slice(1))}`.
pub(crate) fn encode_pkg_name_path(pkg_name: &str) -> String {
    let (prefix, rest) = if let Some(stripped) = pkg_name.strip_prefix('@') {
        ("@", stripped)
    } else {
        ("", pkg_name)
    };
    let mut out = String::with_capacity(prefix.len() + rest.len());
    out.push_str(prefix);
    for byte in rest.bytes() {
        if is_uri_component_unreserved(byte) {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").unwrap();
        }
    }
    out
}

/// Matches JS's `encodeURIComponent` unreserved set:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Anything else gets percent-encoded.
fn is_uri_component_unreserved(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')',
    )
}

#[cfg(test)]
mod tests;

//! Build the `<registry>/<encoded-pkg>` URL for a metadata fetch.
//!
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
//! `@` is preserved while the rest of the name is percent-encoded, and
//! every other character that `encodeURIComponent` would touch is
//! percent-encoded.

use pnpm_network::{encode_package_name, normalize_registry_url};

/// Compose the metadata-fetch URL: `<registry-with-trailing-slash><encoded-name>`.
#[must_use]
pub fn to_registry_url(registry: &str, pkg_name: &str) -> String {
    let registry = normalize_registry_url(registry);
    let encoded = encode_package_name(pkg_name);
    format!("{registry}{encoded}")
}

#[cfg(test)]
mod tests;

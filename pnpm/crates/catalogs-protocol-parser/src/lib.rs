//! Splits the `catalog:` protocol prefix off a manifest bare specifier
//! and returns the requested catalog name.

use pnpm_catalogs_types::DEFAULT_CATALOG_NAME;

const CATALOG_PROTOCOL: &str = "catalog:";

/// Parse a package.json dependency specifier using the `catalog:`
/// protocol.
#[must_use]
pub fn parse_catalog_protocol(bare_specifier: &str) -> Option<&str> {
    let raw = bare_specifier.strip_prefix(CATALOG_PROTOCOL)?.trim();
    Some(if raw.is_empty() { DEFAULT_CATALOG_NAME } else { raw })
}

#[cfg(test)]
mod tests;

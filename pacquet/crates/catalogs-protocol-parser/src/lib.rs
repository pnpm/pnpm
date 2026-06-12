//! Pacquet port of pnpm's
//! [`@pnpm/catalogs.protocol-parser`](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/protocol-parser/src/parseCatalogProtocol.ts).
//!
//! Splits the `catalog:` protocol prefix off a manifest bare specifier
//! and returns the requested catalog name. Used by the resolver chain
//! to decide whether a wanted dependency should be looked up in a
//! catalog before falling through to the npm / git / tarball resolvers.

use pacquet_catalogs_types::DEFAULT_CATALOG_NAME;

const CATALOG_PROTOCOL: &str = "catalog:";

/// Parse a package.json dependency specifier using the `catalog:`
/// protocol.
///
/// Returns `None` if the specifier does not start with `catalog:`.
/// An empty `catalog:` is shorthand for [`DEFAULT_CATALOG_NAME`].
///
/// Mirrors upstream's `parseCatalogProtocol`
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/protocol-parser/src/parseCatalogProtocol.ts#L3-L16)).
#[must_use]
pub fn parse_catalog_protocol(bare_specifier: &str) -> Option<&str> {
    let raw = bare_specifier.strip_prefix(CATALOG_PROTOCOL)?.trim();
    Some(if raw.is_empty() { DEFAULT_CATALOG_NAME } else { raw })
}

#[cfg(test)]
mod tests;

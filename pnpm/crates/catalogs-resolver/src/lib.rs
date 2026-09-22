//! Dereferences a `catalog:` bare specifier against a parsed
//! [`Catalogs`] map and returns either the configured version or one of
//! the misconfiguration errors. The npm-resolver chain calls
//! [`resolve_from_catalog`] before its own protocol dispatch so a
//! resolved [`CatalogResolutionFound::resolution`] feeds back in as a
//! plain bare specifier.

use std::path::Path;

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_local_spec::LocalSpec;

/// Subset of `pnpm-resolving-resolver-base`'s [`WantedDependency`]
/// that catalog resolution needs. Modeled as its own type so this
/// crate doesn't depend on the resolver-base crate; the conversion
/// is a trivial field copy at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedDependency {
    pub alias: String,
    pub bare_specifier: String,
}

/// Which directory a catalog entry's relative path is measured from
/// once resolved.
///
/// A catalog is written in `pnpm-workspace.yaml`, so its relative paths
/// start at the workspace directory, while every consumer reads a
/// specifier relative to itself. Each call site states which of the two
/// it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogAnchor<'a> {
    /// Re-anchor the entry from `workspace_dir` to `consumer_dir`. A
    /// `None` consumer yields the absolute path, for a consumer that
    /// installs somewhere unrelated to the workspace.
    Reanchor { workspace_dir: &'a Path, consumer_dir: Option<&'a Path> },
    /// Return the entry exactly as it is written. For call sites that
    /// compare or display an entry rather than install from it, and for
    /// those that anchor the resolved specifier themselves.
    AsWritten,
}

/// Outcome of [`resolve_from_catalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogResolutionResult {
    /// The catalog protocol resolved to a usable specifier.
    Found(CatalogResolutionFound),
    /// The catalog entry was missing or referenced a catalog itself.
    Misconfiguration(CatalogResolutionMisconfiguration),
    /// The wanted dependency does not use the catalog protocol.
    Unused,
}

/// Successful catalog dereference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogResolutionFound {
    pub resolution: CatalogResolution,
}

/// Resolved (catalog name, specifier) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogResolution {
    /// Catalog the entry was found in.
    pub catalog_name: String,
    /// Version specifier the catalog entry resolved to.
    pub specifier: String,
}

/// A user-misconfigured catalog entry. Carries the error so the call
/// site can rethrow or render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogResolutionMisconfiguration {
    pub catalog_name: String,
    pub error: CatalogResolutionError,
}

/// The ways a `catalog:` lookup can fail. Each variant carries the
/// `pnpm` error code reported for that failure.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatalogResolutionError {
    #[display("No catalog entry '{alias}' was found for catalog '{catalog_name}'.")]
    #[diagnostic(code(ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC))]
    EntryNotFoundForSpec { alias: String, catalog_name: String },

    #[display(
        "Found invalid catalog entry using the catalog protocol recursively. The entry for '{alias}' in catalog '{catalog_name}' is invalid."
    )]
    #[diagnostic(code(ERR_PNPM_CATALOG_ENTRY_INVALID_RECURSIVE_DEFINITION))]
    EntryInvalidRecursiveDefinition { alias: String, catalog_name: String },
}

/// Resolve a wanted dependency through the catalogs map.
#[must_use]
pub fn resolve_from_catalog(
    catalogs: &Catalogs,
    wanted_dependency: &WantedDependency,
    anchor: CatalogAnchor<'_>,
) -> CatalogResolutionResult {
    let Some(catalog_name) = parse_catalog_protocol(&wanted_dependency.bare_specifier) else {
        return CatalogResolutionResult::Unused;
    };

    let catalog_lookup = catalogs
        .get(catalog_name)
        .and_then(|catalog| catalog.get(&wanted_dependency.alias));
    let Some(catalog_lookup) = catalog_lookup else {
        return CatalogResolutionResult::Misconfiguration(CatalogResolutionMisconfiguration {
            catalog_name: catalog_name.to_string(),
            error: CatalogResolutionError::EntryNotFoundForSpec {
                alias: wanted_dependency.alias.clone(),
                catalog_name: catalog_name.to_string(),
            },
        });
    };

    if parse_catalog_protocol(catalog_lookup).is_some() {
        return recursive_catalog_error(catalog_name, &wanted_dependency.alias);
    }

    CatalogResolutionResult::Found(CatalogResolutionFound {
        resolution: CatalogResolution {
            catalog_name: catalog_name.to_string(),
            specifier: anchored_specifier(catalog_lookup, anchor),
        },
    })
}

/// Move an entry naming a local path from the workspace directory to
/// the directory that consumes it. Every other specifier is independent
/// of where it was written, so it passes through.
///
/// A bare path (`./tarballs/x.tgz`) moves with the protocol forms: it
/// means the same thing to the resolver as `file:./tarballs/x.tgz`, so
/// a catalog cannot measure the two from different directories. Only
/// the shapes that can *only* be a local path move; a git shorthand
/// such as `user/repo` is left alone.
fn anchored_specifier(catalog_lookup: &str, anchor: CatalogAnchor<'_>) -> String {
    let CatalogAnchor::Reanchor { workspace_dir, consumer_dir } = anchor else {
        return catalog_lookup.to_string();
    };
    LocalSpec::parse_filesystem(catalog_lookup, workspace_dir)
        .map_or_else(|| catalog_lookup.to_string(), |spec| spec.render(consumer_dir))
}

fn recursive_catalog_error(catalog_name: &str, alias: &str) -> CatalogResolutionResult {
    CatalogResolutionResult::Misconfiguration(CatalogResolutionMisconfiguration {
        catalog_name: catalog_name.to_string(),
        error: CatalogResolutionError::EntryInvalidRecursiveDefinition {
            alias: alias.to_string(),
            catalog_name: catalog_name.to_string(),
        },
    })
}

#[cfg(test)]
mod tests;

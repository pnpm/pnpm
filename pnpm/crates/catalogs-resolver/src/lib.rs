//! Dereferences a `catalog:` bare specifier against a parsed
//! [`Catalogs`] map and returns either the configured version or one of
//! the misconfiguration errors. The npm-resolver chain calls
//! [`resolve_from_catalog`] before its own protocol dispatch so a
//! resolved [`CatalogResolutionFound::resolution`] feeds back in as a
//! plain bare specifier.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;

/// Subset of `pacquet-resolving-resolver-base`'s [`WantedDependency`]
/// that catalog resolution needs. Modeled as its own type so this
/// crate doesn't depend on the resolver-base crate; the conversion
/// is a trivial field copy at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedDependency {
    pub alias: String,
    pub bare_specifier: String,
}

/// Outcome of [`resolve_from_catalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogResolutionResult {
    /// The catalog protocol resolved to a usable specifier.
    Found(CatalogResolutionFound),
    /// The catalog entry was missing or used a forbidden protocol.
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

/// The four ways a `catalog:` lookup can fail. Each variant carries the
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

    #[display(
        "The workspace protocol cannot be used as a catalog value. The entry for '{alias}' in catalog '{catalog_name}' is invalid."
    )]
    #[diagnostic(code(ERR_PNPM_CATALOG_ENTRY_INVALID_WORKSPACE_SPEC))]
    EntryInvalidWorkspaceSpec { alias: String, catalog_name: String },

    #[display(
        "The entry for '{alias}' in catalog '{catalog_name}' declares a dependency using the '{protocol}' protocol. This is not yet supported, but may be in a future version of pnpm."
    )]
    #[diagnostic(code(ERR_PNPM_CATALOG_ENTRY_INVALID_SPEC))]
    EntryInvalidSpec { alias: String, catalog_name: String, protocol: String },
}

/// Resolve a wanted dependency through the catalogs map.
#[must_use]
pub fn resolve_from_catalog(
    catalogs: &Catalogs,
    wanted_dependency: &WantedDependency,
) -> CatalogResolutionResult {
    let Some(catalog_name) = parse_catalog_protocol(&wanted_dependency.bare_specifier) else {
        return CatalogResolutionResult::Unused;
    };

    let catalog_lookup =
        catalogs.get(catalog_name).and_then(|catalog| catalog.get(&wanted_dependency.alias));
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
        return CatalogResolutionResult::Misconfiguration(CatalogResolutionMisconfiguration {
            catalog_name: catalog_name.to_string(),
            error: CatalogResolutionError::EntryInvalidRecursiveDefinition {
                alias: wanted_dependency.alias.clone(),
                catalog_name: catalog_name.to_string(),
            },
        });
    }

    // `workspace:` is banned: it's silly to indirect through a catalog
    // when the workspace protocol resolves directly, and `link:`
    // resolutions cannot be cached in `pnpm-lock.yaml` across importers
    // the way semver selectors can.
    let protocol_of_lookup = catalog_lookup.split(':').next().unwrap_or("");
    if protocol_of_lookup == "workspace" {
        return CatalogResolutionResult::Misconfiguration(CatalogResolutionMisconfiguration {
            catalog_name: catalog_name.to_string(),
            error: CatalogResolutionError::EntryInvalidWorkspaceSpec {
                alias: wanted_dependency.alias.clone(),
                catalog_name: catalog_name.to_string(),
            },
        });
    }

    if matches!(protocol_of_lookup, "link" | "file") {
        return CatalogResolutionResult::Misconfiguration(CatalogResolutionMisconfiguration {
            catalog_name: catalog_name.to_string(),
            error: CatalogResolutionError::EntryInvalidSpec {
                alias: wanted_dependency.alias.clone(),
                catalog_name: catalog_name.to_string(),
                protocol: protocol_of_lookup.to_string(),
            },
        });
    }

    CatalogResolutionResult::Found(CatalogResolutionFound {
        resolution: CatalogResolution {
            catalog_name: catalog_name.to_string(),
            specifier: catalog_lookup.clone(),
        },
    })
}

#[cfg(test)]
mod tests;

//! Pacquet port of pnpm's
//! [`@pnpm/catalogs.config`](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/config/src/getCatalogsFromWorkspaceManifest.ts).
//!
//! Normalizes the two surface forms `pnpm-workspace.yaml` supports for
//! defining the default catalog — `catalog:` at the top level vs.
//! `catalogs.default` nested under the named catalogs — into the
//! single flat [`Catalogs`] map every resolver consumer expects.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pacquet_workspace::WorkspaceManifest;

/// Raised when the workspace manifest defines the default catalog
/// twice — once via the top-level `catalog:` shorthand and once via
/// the explicit `catalogs.default` key.
///
/// Mirrors upstream's `INVALID_CATALOGS_CONFIGURATION` `PnpmError`
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/config/src/getCatalogsFromWorkspaceManifest.ts#L32-L37)).
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidCatalogsConfigurationError {
    #[display(
        "The 'default' catalog was defined multiple times. Use the 'catalog' field or 'catalogs.default', but not both."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_CATALOGS_CONFIGURATION))]
    DefaultDefinedMultipleTimes,
}

/// Project the catalog-shaped fields from a parsed workspace manifest
/// into a single flat [`Catalogs`] map.
///
/// Mirrors upstream's `getCatalogsFromWorkspaceManifest`
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/config/src/getCatalogsFromWorkspaceManifest.ts#L5-L30)).
pub fn get_catalogs_from_workspace_manifest(
    workspace_manifest: Option<&WorkspaceManifest>,
) -> Result<Catalogs, InvalidCatalogsConfigurationError> {
    let Some(manifest) = workspace_manifest else {
        return Ok(Catalogs::new());
    };

    check_default_catalog_is_defined_once(manifest)?;

    // Upstream spreads `workspace.catalogs` after writing `default`, so
    // an explicit `catalogs.default` overrides the (already-validated
    // to be absent) `catalog` field. With `catalog`/`catalogs.default`
    // mutually exclusive only one branch ever populates the key.
    let mut catalogs = Catalogs::new();
    if let Some(default) = &manifest.catalog {
        catalogs.insert(DEFAULT_CATALOG_NAME.to_string(), default.clone());
    }
    if let Some(named) = &manifest.catalogs {
        for (name, catalog) in named {
            catalogs.insert(name.clone(), catalog.clone());
        }
    }

    Ok(catalogs)
}

/// Validate that the default catalog is defined through at most one of
/// the two surface forms.
///
/// Mirrors upstream's `checkDefaultCatalogIsDefinedOnce`
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/config/src/getCatalogsFromWorkspaceManifest.ts#L32-L40)).
pub fn check_default_catalog_is_defined_once(
    manifest: &WorkspaceManifest,
) -> Result<(), InvalidCatalogsConfigurationError> {
    if manifest.catalog.is_some()
        && manifest.catalogs.as_ref().is_some_and(|c| c.contains_key(DEFAULT_CATALOG_NAME))
    {
        return Err(InvalidCatalogsConfigurationError::DefaultDefinedMultipleTimes);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

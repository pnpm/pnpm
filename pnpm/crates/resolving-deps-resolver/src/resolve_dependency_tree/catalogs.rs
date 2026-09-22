//! `catalog:` specifier resolution for importer-level dependencies.

use std::path::Path;

use pnpm_catalogs_resolver::{
    CatalogAnchor,
    CatalogResolutionResult,
    WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;

use super::{
    ResolveDependencyTreeError,
    WantedSpec,
};

/// The anchor for an entry the manifest in `consumer_dir` dereferences.
/// An install with no `pnpm-workspace.yaml` declares no catalogs, so
/// there is no path to move.
pub(super) fn catalog_anchor<'a>(
    workspace_dir: Option<&'a Path>,
    consumer_dir: Option<&'a Path>,
) -> CatalogAnchor<'a> {
    match workspace_dir {
        Some(workspace_dir) => CatalogAnchor::Reanchor { workspace_dir, consumer_dir },
        None => CatalogAnchor::AsWritten,
    }
}

/// Replace `catalog:` bare specifiers on direct dependencies with the
/// version recorded in the catalogs map. Non-`catalog:` specifiers
/// pass through unchanged.
///
/// Catalog resolution runs only on importer-level deps. A misconfigured
/// entry surfaces immediately rather than masquerading as a
/// `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
pub(crate) fn resolve_catalog_specifiers(
    specs: Vec<WantedSpec>,
    catalogs: &Catalogs,
    anchor: CatalogAnchor<'_>,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError> {
    specs
        .into_iter()
        .map(|(name, range, optional, injected)| {
            resolve_catalog_specifier(name, range, catalogs, anchor)
                .map(|(name, range)| (name, range, optional, injected))
        })
        .collect()
}

pub(super) fn resolve_catalog_specifier(
    name: String,
    range: String,
    catalogs: &Catalogs,
    anchor: CatalogAnchor<'_>,
) -> Result<(String, String), ResolveDependencyTreeError> {
    let wanted = CatalogWantedDependency { alias: name.clone(), bare_specifier: range.clone() };
    match resolve_from_catalog(catalogs, &wanted, anchor) {
        CatalogResolutionResult::Found(found) => Ok((name, found.resolution.specifier)),
        CatalogResolutionResult::Unused => Ok((name, range)),
        CatalogResolutionResult::Misconfiguration(misconfig) => {
            Err(ResolveDependencyTreeError::CatalogMisconfiguration(misconfig.error))
        }
    }
}

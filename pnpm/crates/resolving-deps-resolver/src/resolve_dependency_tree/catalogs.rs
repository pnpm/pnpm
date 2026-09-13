//! `catalog:` specifier resolution for importer-level dependencies.

use pnpm_catalogs_resolver::{
    CatalogResolutionResult, WantedDependency as CatalogWantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;

use super::{ResolveDependencyTreeError, WantedSpec};

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
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError> {
    specs
        .into_iter()
        .map(|(name, range, optional, injected)| {
            resolve_catalog_specifier(name, range, catalogs)
                .map(|(name, range)| (name, range, optional, injected))
        })
        .collect()
}

pub(super) fn resolve_catalog_specifier(
    name: String,
    range: String,
    catalogs: &Catalogs,
) -> Result<(String, String), ResolveDependencyTreeError> {
    let wanted = CatalogWantedDependency { alias: name.clone(), bare_specifier: range.clone() };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok((name, found.resolution.specifier)),
        CatalogResolutionResult::Unused => Ok((name, range)),
        CatalogResolutionResult::Misconfiguration(misconfig) => {
            Err(ResolveDependencyTreeError::CatalogMisconfiguration(misconfig.error))
        }
    }
}

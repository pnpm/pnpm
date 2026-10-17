//! `catalog:` specifier resolution for importer-level dependencies.

use std::path::Path;

use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;

use super::{ResolveDependencyTreeError, WantedSpec};

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

/// Where the `catalog:` specs of an injected workspace project resolve.
pub struct ChildCatalogs<'a> {
    pub catalogs: &'a Catalogs,
    /// Where `pnpm-workspace.yaml` sits.
    pub workspace_dir: Option<&'a Path>,
    /// The project's directory, which declares the specs.
    pub package_dir: &'a Path,
}

/// The child specs the resolver walks for a package resolved from a local
/// directory: [`manifest_child_specs`], with `catalog:` specs resolved
/// through `catalogs` when the package is a workspace project, as the
/// walk resolves them.
///
/// [`manifest_child_specs`]: crate::manifest_child_specs
pub fn local_directory_child_specs(
    manifest: &serde_json::Value,
    parent: &str,
    catalogs: Option<ChildCatalogs<'_>>,
) -> Result<Vec<super::ChildSpec>, ResolveDependencyTreeError> {
    let specs = super::manifest::manifest_child_specs(manifest, parent)?;
    match catalogs {
        Some(catalogs) if !catalogs.catalogs.is_empty() => resolve_catalog_specifiers(
            specs,
            catalogs.catalogs,
            catalog_anchor(catalogs.workspace_dir, Some(catalogs.package_dir)),
        ),
        _ => Ok(specs),
    }
}

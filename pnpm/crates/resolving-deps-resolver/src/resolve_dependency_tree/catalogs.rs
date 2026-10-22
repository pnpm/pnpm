//! `catalog:` specifier resolution for importer-level dependencies.

use std::path::Path;

use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;

use super::{DependencySpec, ResolveDependencyTreeError};

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
    specs: Vec<DependencySpec>,
    catalogs: &Catalogs,
    anchor: CatalogAnchor<'_>,
) -> Result<Vec<DependencySpec>, ResolveDependencyTreeError> {
    specs
        .into_iter()
        .map(|spec| {
            resolve_catalog_specifier(spec.alias, spec.bare_specifier, catalogs, anchor)
                .map(|(alias, bare_specifier)| DependencySpec {
                    alias,
                    bare_specifier,
                    optional: spec.optional,
                    injected: spec.injected,
                })
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

/// What the walk does with a package's child specs before resolving them:
/// it drops the required children that the package's own peers supply
/// (`is_peer_shadowed`), then resolves `catalog:` specs.
pub(super) fn walked_child_specs(
    specs: &[DependencySpec],
    is_peer_shadowed: impl Fn(&str) -> bool,
    catalogs: Option<(&Catalogs, CatalogAnchor<'_>)>,
) -> Result<Vec<DependencySpec>, ResolveDependencyTreeError> {
    let walked = specs
        .iter()
        .filter(|spec| spec.optional || !is_peer_shadowed(&spec.alias))
        .cloned()
        .collect();
    match catalogs {
        Some((catalogs, anchor)) => resolve_catalog_specifiers(walked, catalogs, anchor),
        None => Ok(walked),
    }
}

/// The child specs the resolver walks for a package resolved from a local
/// directory: [`manifest_child_specs`], less the required children its own
/// peers supply (`is_peer_shadowed`), with `catalog:` specs resolved through
/// `catalogs` when the package is a workspace project.
///
/// [`manifest_child_specs`]: crate::manifest_child_specs
pub fn local_directory_child_specs(
    manifest: &serde_json::Value,
    parent: &str,
    is_peer_shadowed: impl Fn(&str) -> bool,
    catalogs: Option<ChildCatalogs<'_>>,
) -> Result<Vec<DependencySpec>, ResolveDependencyTreeError> {
    let specs = super::manifest::manifest_child_specs(manifest, parent)?;
    let catalogs = catalogs
        .filter(|catalogs| !catalogs.catalogs.is_empty())
        .map(|catalogs| {
            let anchor = catalog_anchor(catalogs.workspace_dir, Some(catalogs.package_dir));
            (catalogs.catalogs, anchor)
        });
    walked_child_specs(&specs, is_peer_shadowed, catalogs)
}

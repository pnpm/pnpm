use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pnpm_config_parse_overrides::parse_pkg_and_parent_selector;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};

use super::{Manifest, edit::CatalogReferences};

/// What the `catalogPrune` pass counts as a reference to a catalog entry.
#[derive(Default)]
pub struct CatalogReferenceSources<'a> {
    /// Every workspace project manifest (with in-memory dependency edits
    /// applied), consulted by the cleanup pass to decide which catalog
    /// entries are still referenced. An empty list disables the cleanup
    /// pass, mirroring upstream's `allProjects ?? []` guard.
    pub all_projects: &'a [&'a PackageManifest],
    /// Catalog entries the cleanup pass keeps even when no manifest in
    /// [`Self::all_projects`] references them.
    pub kept_catalogs: Option<&'a Catalogs>,
}

impl CatalogReferenceSources<'_> {
    /// The upstream `packageReferences` map: every raw dependency specifier
    /// per package name across `dependencies`, `devDependencies`,
    /// `optionalDependencies`, and `peerDependencies` of every project, plus
    /// the workspace manifest's own `catalog:`-valued `overrides:` (whose
    /// selector names the referenced package). Selectors that fail to parse
    /// are skipped, matching upstream. Each entry of
    /// [`Self::kept_catalogs`] adds the reference that keeps it.
    pub(crate) fn collect(&self, manifest: &Manifest) -> CatalogReferences {
        const GROUPS: [DependencyGroup; 4] = [
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ];
        let mut references = CatalogReferences::new();
        for project in self.all_projects {
            for (name, specifier) in project.dependencies(GROUPS) {
                references
                    .entry(name.to_string())
                    .or_default()
                    .insert(specifier.to_string());
            }
        }
        for (selector, specifier) in manifest.overrides.iter().flatten() {
            if !specifier.starts_with("catalog:") {
                continue;
            }
            let Ok((_, target_pkg)) = parse_pkg_and_parent_selector(selector) else {
                continue;
            };
            references
                .entry(target_pkg.name)
                .or_default()
                .insert(specifier.clone());
        }
        if let Some(kept_catalogs) = self.kept_catalogs {
            add_kept_catalog_references(&mut references, kept_catalogs);
        }
        references
    }
}

fn add_kept_catalog_references(references: &mut CatalogReferences, kept_catalogs: &Catalogs) {
    for (catalog_name, entries) in kept_catalogs {
        let specifier = if catalog_name == DEFAULT_CATALOG_NAME {
            "catalog:".to_string()
        } else {
            format!("catalog:{catalog_name}")
        };
        for alias in entries.keys() {
            references
                .entry(alias.clone())
                .or_default()
                .insert(specifier.clone());
        }
    }
}

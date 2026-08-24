use crate::{PkgName, ProjectSnapshot, freshness::auto_installed_peer_deps};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashSet;

/// Drop the entries of `importer` that `manifest` does not declare and
/// `pre_merge` did not already carry, per dependency group. Reports
/// whether anything was dropped.
///
/// `pre_merge` is the same importer before the git branch lockfiles were
/// folded in. The fold unions keys, so it can only ever add, and only
/// what it added may be taken back out: an undeclared entry the read file
/// already held is drift the freshness check owns, not the fold's to
/// repair.
///
/// Which group a manifest entry belongs to is derived exactly the way
/// [`crate::satisfies_package_manifest`] derives it, so pruning to the
/// manifest hands that check the fields it expects to find.
pub fn prune_undeclared_importer_deps(
    importer: &mut ProjectSnapshot,
    pre_merge: Option<&ProjectSnapshot>,
    manifest: &PackageManifest,
    auto_install_peers: bool,
) -> bool {
    let declared = DeclaredNames::of(manifest, auto_install_peers);
    let mut pruned = false;
    for (declared_in_group, group, field) in [
        (&declared.prod, DependencyGroup::Prod, &mut importer.dependencies),
        (&declared.dev, DependencyGroup::Dev, &mut importer.dev_dependencies),
        (&declared.optional, DependencyGroup::Optional, &mut importer.optional_dependencies),
    ] {
        let Some(dependencies) = field.as_mut() else { continue };
        let before = dependencies.len();
        dependencies.retain(|name, _| {
            declared_in_group.contains(name)
                || pre_merge
                    .and_then(|pre| pre.get_map_by_group(group))
                    .is_some_and(|pre| pre.contains_key(name))
        });
        pruned |= dependencies.len() != before;
        if dependencies.is_empty() {
            *field = None;
        }
    }
    if let Some(specifiers) = importer.specifiers.as_mut() {
        specifiers.retain(|name, _| {
            PkgName::parse(name.as_str()).is_ok_and(|parsed| declared.contains_anywhere(&parsed))
                || pre_merge
                    .and_then(|pre| pre.specifiers.as_ref())
                    .is_some_and(|pre| pre.contains_key(name))
        });
    }
    pruned
}

/// The manifest's dependency names split into the importer groups they
/// are recorded under.
struct DeclaredNames {
    prod: HashSet<PkgName>,
    dev: HashSet<PkgName>,
    optional: HashSet<PkgName>,
}

impl DeclaredNames {
    /// Precedence for an alias several groups declare: optional over
    /// prod, prod over dev.
    fn of(manifest: &PackageManifest, auto_install_peers: bool) -> Self {
        let names_of = |group| {
            manifest.dependencies([group]).filter_map(|(name, _)| PkgName::parse(name).ok())
        };
        let optional: HashSet<PkgName> = names_of(DependencyGroup::Optional).collect();
        let mut prod: HashSet<PkgName> =
            names_of(DependencyGroup::Prod).filter(|name| !optional.contains(name)).collect();
        let dev: HashSet<PkgName> = names_of(DependencyGroup::Dev)
            .filter(|name| !optional.contains(name) && !prod.contains(name))
            .collect();
        prod.extend(
            auto_installed_peer_deps(manifest, auto_install_peers)
                .into_keys()
                .filter_map(|name| PkgName::parse(name).ok()),
        );
        DeclaredNames { prod, dev, optional }
    }

    fn contains_anywhere(&self, name: &PkgName) -> bool {
        self.prod.contains(name) || self.dev.contains(name) || self.optional.contains(name)
    }
}

#[cfg(test)]
mod tests;

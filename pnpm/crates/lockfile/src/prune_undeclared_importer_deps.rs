use crate::{PkgName, ProjectSnapshot, freshness::auto_installed_peer_deps};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashSet;

/// Drop every entry of `importer` that `manifest` no longer declares,
/// per dependency group. For the callers holding a lockfile no
/// resolution has reconciled against the manifests: the merge of the git
/// branch lockfiles unions their keys, so it can only ever add.
///
/// Which group a manifest entry belongs to is derived exactly the way
/// [`crate::satisfies_package_manifest`] derives it, so pruning to the
/// manifest hands that check the fields it expects to find.
pub fn prune_undeclared_importer_deps(
    importer: &mut ProjectSnapshot,
    manifest: &PackageManifest,
    auto_install_peers: bool,
) {
    let declared = DeclaredNames::of(manifest, auto_install_peers);
    for (declared_in_group, field) in [
        (&declared.prod, &mut importer.dependencies),
        (&declared.dev, &mut importer.dev_dependencies),
        (&declared.optional, &mut importer.optional_dependencies),
    ] {
        let Some(dependencies) = field.as_mut() else { continue };
        dependencies.retain(|name, _| declared_in_group.contains(name));
        if dependencies.is_empty() {
            *field = None;
        }
    }
    if let Some(specifiers) = importer.specifiers.as_mut() {
        specifiers.retain(|name, _| {
            PkgName::parse(name.as_str()).is_ok_and(|name| declared.contains_anywhere(&name))
        });
    }
}

/// The manifest's dependency names split into the importer groups they
/// are recorded under.
struct DeclaredNames {
    prod: HashSet<PkgName>,
    dev: HashSet<PkgName>,
    optional: HashSet<PkgName>,
}

impl DeclaredNames {
    /// An alias the manifest declares in several groups belongs to the
    /// most specific one: optional wins over prod, prod over dev. An
    /// auto-installed peer joins `dependencies`, but only when no other
    /// group declares it — see [`auto_installed_peer_deps`].
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

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

/// Reconcile importer metadata (`publishDirectory`, `linkDirectory`, and
/// `dependenciesMeta`) back to `manifest` wherever the branch-lockfile fold
/// changed what `pre_merge` carried. Reports whether any field changed.
///
/// Same rule as [`prune_undeclared_importer_deps`]: only differences the fold
/// introduced relative to `pre_merge` may be reconciled, so drift the read
/// lockfile already held remains visible to [`crate::satisfies_package_manifest`].
pub fn reconcile_merged_importer_metadata(
    importer: &mut ProjectSnapshot,
    pre_merge: Option<&ProjectSnapshot>,
    manifest: &PackageManifest,
) -> bool {
    let publish_changed = reconcile_publish_config(importer, pre_merge, manifest);
    let meta_changed = reconcile_dependencies_meta(importer, pre_merge, manifest);
    publish_changed || meta_changed
}

fn reconcile_publish_config(
    importer: &mut ProjectSnapshot,
    pre_merge: Option<&ProjectSnapshot>,
    manifest: &PackageManifest,
) -> bool {
    let publish_config = manifest.value().get("publishConfig");
    let manifest_publish_dir = publish_config
        .and_then(|config| config.get("directory"))
        .and_then(serde_json::Value::as_str);
    let mut changed = false;
    if importer.publish_directory.as_deref() != manifest_publish_dir
        && pre_merge.and_then(|pre| pre.publish_directory.as_deref()) == manifest_publish_dir
    {
        importer.publish_directory = manifest_publish_dir.map(str::to_owned);
        changed = true;
    }
    let manifest_link_dir = manifest_publish_dir
        .is_some()
        .then(|| {
            publish_config
                .and_then(|config| config.get("linkDirectory"))
                .and_then(serde_json::Value::as_bool)
                .filter(|link_directory| !link_directory)
        })
        .flatten();
    let pre_link_dir = pre_merge.and_then(|pre| {
        pre.publish_directory
            .as_ref()
            .and(pre.link_directory)
            .filter(|link_directory| !link_directory)
    });
    if importer.link_directory != manifest_link_dir && pre_link_dir == manifest_link_dir {
        importer.link_directory = manifest_link_dir;
        changed = true;
    }
    changed
}

fn reconcile_dependencies_meta(
    importer: &mut ProjectSnapshot,
    pre_merge: Option<&ProjectSnapshot>,
    manifest: &PackageManifest,
) -> bool {
    let Some(serde_json::Value::Object(merged_map)) = importer.dependencies_meta.as_mut() else {
        return false;
    };
    let manifest_map = manifest
        .value()
        .get("dependenciesMeta")
        .and_then(serde_json::Value::as_object);
    let pre_map = pre_merge
        .and_then(|pre| pre.dependencies_meta.as_ref())
        .and_then(serde_json::Value::as_object);
    let mut changed = false;
    merged_map.retain(|dep_name, merged_entry| {
        let Some(manifest_entry) = manifest_map.and_then(|map| map.get(dep_name)) else {
            let keep = pre_map.is_some_and(|map| map.contains_key(dep_name));
            changed |= !keep;
            return keep;
        };
        let pre_entry = pre_map.and_then(|map| map.get(dep_name));
        changed |= reconcile_meta_entry(merged_entry, pre_entry, manifest_entry);
        true
    });
    if merged_map.is_empty() {
        importer.dependencies_meta = None;
        changed = true;
    }
    changed
}

fn reconcile_meta_entry(
    merged_entry: &mut serde_json::Value,
    pre_entry: Option<&serde_json::Value>,
    manifest_entry: &serde_json::Value,
) -> bool {
    let (serde_json::Value::Object(merged_props), serde_json::Value::Object(manifest_props)) =
        (merged_entry, manifest_entry)
    else {
        return false;
    };
    let pre_props = pre_entry.and_then(serde_json::Value::as_object);
    let mut changed = false;
    merged_props.retain(|key, merged_val| {
        let Some(manifest_val) = manifest_props.get(key) else {
            let keep = pre_props.is_some_and(|props| props.contains_key(key));
            changed |= !keep;
            return keep;
        };
        if merged_val != manifest_val
            && pre_props.and_then(|props| props.get(key)) == Some(manifest_val)
        {
            *merged_val = manifest_val.clone();
            changed = true;
        }
        true
    });
    changed
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
            manifest
                .dependencies([group])
                .filter_map(|(name, _)| PkgName::parse(name).ok())
        };
        let optional: HashSet<PkgName> = names_of(DependencyGroup::Optional).collect();
        let mut prod: HashSet<PkgName> = names_of(DependencyGroup::Prod)
            .filter(|name| !optional.contains(name))
            .collect();
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

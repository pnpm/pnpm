//! The importers a previous wanted lockfile records for projects outside the
//! workspace walk, such as projects a sparse checkout leaves off disk.

use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_lockfile::{
    Lockfile,
    ProjectSnapshot,
};
use pnpm_package_manifest::DependencyGroup;

/// The importers `previous_wanted` records outside `real_importer_ids`. Their
/// catalog entries are copied into `lockfile`, where it recorded none.
pub(super) fn retain_importers(
    previous_wanted: Option<&Lockfile>,
    real_importer_ids: &HashSet<String>,
    workspace_root: &std::path::Path,
    lockfile: &mut Lockfile,
) -> HashMap<String, ProjectSnapshot> {
    let importers = previous_wanted
        .into_iter()
        .flat_map(|previous| missing_project_importers(previous, real_importer_ids))
        .map(|(importer_id, importer)| (importer_id.clone(), importer.clone()))
        .collect();
    retain_catalog_entries(lockfile, previous_wanted, &importers, workspace_root);
    importers
}

/// The importers `lockfile` records outside `real_importer_ids`.
pub fn missing_project_importers<'a>(
    lockfile: &'a Lockfile,
    real_importer_ids: &'a HashSet<String>,
) -> impl Iterator<Item = (&'a String, &'a ProjectSnapshot)> {
    lockfile.importers
        .iter()
        .filter(|(importer_id, _)| !real_importer_ids.contains(*importer_id))
}

/// Copy the catalog entries `importers` use from `previous_wanted` into
/// `lockfile`, where the fresh resolution recorded none.
fn retain_catalog_entries(
    lockfile: &mut Lockfile,
    previous_wanted: Option<&Lockfile>,
    importers: &HashMap<String, ProjectSnapshot>,
    workspace_root: &std::path::Path,
) {
    if importers.is_empty() {
        return;
    }
    let Some(previous) = previous_wanted else { return };
    let Some(previous_catalogs) = &previous.catalogs else { return };
    let reachable = super::collect_reachable(
        previous,
        workspace_root,
        &importers.keys().cloned().collect(),
        &super::GroupSelection::all(),
        |_| false,
    );
    let packages = reachable.snapshot_keys
        .iter()
        .filter_map(|key| {
            previous.packages
                .as_ref()?
                .get(&key.without_peer())
        });
    let peers =
        directory_peer_catalog_references(packages).map(|(name, alias)| (name, alias.to_owned()));
    for (catalog_name, alias) in importer_catalog_references(importers).chain(peers) {
        let Some(entry) = previous_catalogs
            .get(catalog_name)
            .and_then(|catalog| catalog.get(&alias))
        else {
            continue;
        };
        lockfile.catalogs
            .get_or_insert_default()
            .entry(catalog_name.to_string())
            .or_default()
            .entry(alias)
            .or_insert_with(|| entry.clone());
    }
}

fn importer_catalog_references(
    importers: &HashMap<String, ProjectSnapshot>,
) -> impl Iterator<Item = (&str, String)> {
    importers
        .values()
        .flat_map(|importer| {
            importer.dependencies_by_groups([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ])
        })
        .filter_map(|(alias, dependency)| {
            let catalog_name = parse_catalog_protocol(&dependency.specifier)?;
            Some((catalog_name, alias.to_string()))
        })
}

/// The catalog entries referenced by peers of directory packages.
pub fn directory_peer_catalog_references<'a>(
    packages: impl Iterator<Item = &'a pnpm_lockfile::PackageMetadata>,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    packages
        .filter(|package| {
            matches!(package.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
        })
        .filter_map(|package| package.peer_dependencies.as_ref())
        .flatten()
        .filter_map(|(alias, spec)| {
            Some((pnpm_catalogs_protocol_parser::parse_catalog_protocol(spec)?, alias.as_str()))
        })
}

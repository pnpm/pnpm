//! The importers a previous wanted lockfile records for projects outside the
//! workspace walk, such as projects a sparse checkout leaves off disk.

use std::collections::{HashMap, HashSet};

use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_lockfile::{Lockfile, ProjectSnapshot};
use pnpm_package_manifest::DependencyGroup;

/// The importers `previous_wanted` records outside `real_importer_ids`. Their
/// catalog entries are copied into `lockfile`, where it recorded none.
pub(super) fn retain_importers(
    previous_wanted: Option<&Lockfile>,
    real_importer_ids: &HashSet<String>,
    lockfile: &mut Lockfile,
) -> HashMap<String, ProjectSnapshot> {
    let importers = previous_wanted
        .into_iter()
        .flat_map(|previous| missing_project_importers(previous, real_importer_ids))
        .map(|(importer_id, importer)| (importer_id.clone(), importer.clone()))
        .collect();
    retain_catalog_entries(lockfile, previous_wanted, &importers);
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
) {
    let Some(previous_catalogs) =
        previous_wanted.and_then(|previous| previous.catalogs.as_ref())
    else {
        return;
    };
    let used_entries = importers
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
            let alias = alias.to_string();
            let entry = previous_catalogs.get(catalog_name)?.get(&alias)?;
            Some((catalog_name, alias, entry))
        });
    for (catalog_name, alias, entry) in used_entries {
        lockfile.catalogs
            .get_or_insert_default()
            .entry(catalog_name.to_string())
            .or_default()
            .entry(alias)
            .or_insert_with(|| entry.clone());
    }
}

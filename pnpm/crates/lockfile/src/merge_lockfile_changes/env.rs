use crate::{
    EnvImporterSnapshot, EnvLockfile, PackageKey, PackageMetadata, SpecifierAndResolution,
};
use std::collections::{BTreeMap, HashMap};

use super::{Winner, merge_snapshot, spread, take_changed, winner};

/// [`crate::merge_lockfile_changes()`] for the env document — the config and
/// package-manager dependencies pnpm records ahead of the project
/// lockfile. Two branches that each added a config dependency conflict
/// here rather than in the main document, and the same rules apply:
/// union the entries, prefer the higher version where both sides
/// resolved one.
#[must_use]
pub fn merge_env_lockfile_changes(ours: &EnvLockfile, theirs: &EnvLockfile) -> EnvLockfile {
    EnvLockfile {
        lockfile_version: take_changed(&ours.lockfile_version, &theirs.lockfile_version),
        importers: merge_env_importers(&ours.importers, &theirs.importers),
        packages: merge_plain_map(&ours.packages, &theirs.packages, spread::<PackageMetadata>),
        snapshots: merge_plain_map(&ours.snapshots, &theirs.snapshots, merge_snapshot),
    }
}

fn merge_env_importers(
    ours: &HashMap<String, EnvImporterSnapshot>,
    theirs: &HashMap<String, EnvImporterSnapshot>,
) -> HashMap<String, EnvImporterSnapshot> {
    let mut merged: HashMap<String, EnvImporterSnapshot> = HashMap::new();
    for id in ours.keys().chain(theirs.keys()) {
        if merged.contains_key(id) {
            continue;
        }
        let (our_importer, their_importer) = (ours.get(id), theirs.get(id));
        merged.insert(
            id.clone(),
            EnvImporterSnapshot {
                config_dependencies: merge_specifier_map(
                    our_importer.map(|importer| &importer.config_dependencies),
                    their_importer.map(|importer| &importer.config_dependencies),
                ),
                package_manager_dependencies: merge_optional_specifier_map(
                    our_importer.and_then(|importer| {
                        importer.package_manager_dependencies.as_ref()
                    }),
                    their_importer.and_then(|importer| {
                        importer.package_manager_dependencies.as_ref()
                    }),
                ),
            },
        );
    }
    merged
}

fn merge_specifier_map(
    ours: Option<&BTreeMap<String, SpecifierAndResolution>>,
    theirs: Option<&BTreeMap<String, SpecifierAndResolution>>,
) -> BTreeMap<String, SpecifierAndResolution> {
    let mut merged = ours.cloned().unwrap_or_default();
    for (name, theirs) in theirs.cloned().unwrap_or_default() {
        let entry = match merged.get(&name) {
            Some(ours) => merge_specifier_and_resolution(ours, &theirs),
            None => theirs,
        };
        merged.insert(name, entry);
    }
    merged
}

/// [`merge_specifier_map`] for a group pnpm omits when empty, so a merge
/// of two absent groups stays absent.
fn merge_optional_specifier_map(
    ours: Option<&BTreeMap<String, SpecifierAndResolution>>,
    theirs: Option<&BTreeMap<String, SpecifierAndResolution>>,
) -> Option<BTreeMap<String, SpecifierAndResolution>> {
    if ours.is_none() && theirs.is_none() {
        return None;
    }
    Some(merge_specifier_map(ours, theirs))
}

fn merge_specifier_and_resolution(
    ours: &SpecifierAndResolution,
    theirs: &SpecifierAndResolution,
) -> SpecifierAndResolution {
    SpecifierAndResolution {
        specifier: take_changed(&ours.specifier, &theirs.specifier),
        version: match winner(&ours.version, &theirs.version) {
            Winner::Ours => ours.version.clone(),
            Winner::Theirs => theirs.version.clone(),
        },
    }
}

/// [`super::merge_maps`] for a map the env document always records, so there is
/// no absent case to collapse to `None`.
fn merge_plain_map<Value: Clone>(
    ours: &HashMap<PackageKey, Value>,
    theirs: &HashMap<PackageKey, Value>,
    merge: impl Fn(&Value, &Value) -> Value,
) -> HashMap<PackageKey, Value> {
    let mut merged = ours.clone();
    for (key, their_value) in theirs.clone() {
        let value = match merged.get(&key) {
            Some(our_value) => merge(our_value, &their_value),
            None => their_value,
        };
        merged.insert(key, value);
    }
    merged
}

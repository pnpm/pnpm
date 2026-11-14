//! Rebuild identities after pruning without changing the retained graph.

use super::{
    NextFormatError, RestoreContext, read,
    write::{Explicit, MAX_ROUNDS, check_round_trip, same_entry, snapshots::Snapshots},
};
use crate::{
    ImporterDepVersion, Lockfile, PackageKey, PkgName, ProjectSnapshot, SnapshotDepRef,
    SnapshotEntry,
};
use std::collections::{HashMap, hash_map::Entry};

type Rekeys = HashMap<PackageKey, PackageKey>;

pub(super) fn normalize(lockfile: &Lockfile) -> Result<Lockfile, NextFormatError> {
    let empty = HashMap::new();
    let snapshots =
        Snapshots::of(lockfile.snapshots.as_ref().unwrap_or(&empty), lockfile.packages.as_ref())?;
    let context = RestoreContext {
        packages: lockfile.packages.as_ref(),
        settings: lockfile.settings.as_ref(),
        patched_dependencies: lockfile.patched_dependencies.as_ref(),
    };
    let mut explicit = Explicit::for_normalization(lockfile);
    snapshots.mark_nested_bindings(&mut explicit);
    for _ in 0..MAX_ROUNDS {
        let sections = snapshots.sections(lockfile, &explicit)?;
        let Some(rebuilt) = read::Rebuilt::of(&sections, context) else {
            return Ok(lockfile.clone());
        };
        if snapshots.mark_mismatches(lockfile, &rebuilt, &mut explicit) {
            continue;
        }
        let rekeys = snapshots.rebuilt_keys(&rebuilt);
        let mut expected = rekey_lockfile(lockfile, &rekeys)?;
        let restored = rebuilt.into_sections(&sections, context);
        check_round_trip(&expected, &restored)?;
        refresh_recorded_bindings(&mut expected, restored.snapshots);
        expected.to_next_sections()?;
        return Ok(expected);
    }
    Err(NextFormatError::NotRepresentable {
        detail: "the references did not settle while rebuilding peer keys".to_string(),
    })
}

fn refresh_recorded_bindings(
    expected: &mut Lockfile,
    rebuilt: Option<HashMap<PackageKey, SnapshotEntry>>,
) {
    let Some(snapshots) = expected.snapshots.as_mut() else { return };
    for (key, entry) in rebuilt.into_iter().flatten() {
        if let Some(snapshot) = snapshots.get_mut(&key) {
            snapshot.recorded_peer_bindings = entry.recorded_peer_bindings;
        }
    }
}

fn rekey_lockfile(lockfile: &Lockfile, rekeys: &Rekeys) -> Result<Lockfile, NextFormatError> {
    let mut expected = lockfile.clone();
    for importer in expected.importers.values_mut() {
        rekey_importer(importer, rekeys);
    }
    let Some(snapshots) = expected.snapshots.take() else { return Ok(expected) };
    let mut moved = HashMap::with_capacity(snapshots.len());
    for (key, mut entry) in snapshots {
        rekey_dependencies(&mut entry.dependencies, rekeys);
        rekey_dependencies(&mut entry.optional_dependencies, rekeys);
        let key = rekeys.get(&key).cloned().unwrap_or(key);
        insert_snapshot(&mut moved, key, entry)?;
    }
    expected.snapshots = Some(moved);
    Ok(expected)
}

fn insert_snapshot(
    snapshots: &mut HashMap<PackageKey, SnapshotEntry>,
    key: PackageKey,
    snapshot: SnapshotEntry,
) -> Result<(), NextFormatError> {
    match snapshots.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(snapshot);
        }
        Entry::Occupied(entry) if !same_entry(entry.get(), &snapshot) => {
            return Err(NextFormatError::NotRepresentable {
                detail: format!("unequal snapshots would share the rebuilt key {}", entry.key()),
            });
        }
        Entry::Occupied(_) => {}
    }
    Ok(())
}

fn rekey_dependencies(entries: &mut Option<HashMap<PkgName, SnapshotDepRef>>, rekeys: &Rekeys) {
    for (alias, reference) in entries.iter_mut().flatten() {
        let Some(moved) = reference
            .resolve(alias)
            .and_then(|key| rekeys.get(&key))
        else {
            continue;
        };
        match reference {
            SnapshotDepRef::Plain(version) => *version = moved.suffix.clone(),
            SnapshotDepRef::Alias(key) => *key = moved.clone(),
            SnapshotDepRef::Link(_) => {}
        }
    }
}

fn rekey_importer(importer: &mut ProjectSnapshot, rekeys: &Rekeys) {
    for entries in [
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
        importer.optional_dependencies.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        for (alias, dependency) in entries {
            rekey_importer_version(alias, &mut dependency.version, rekeys);
        }
    }
}

fn rekey_importer_version(alias: &PkgName, version: &mut ImporterDepVersion, rekeys: &Rekeys) {
    let Some(moved) = version
        .resolved_key(alias)
        .and_then(|key| rekeys.get(&key))
    else {
        return;
    };
    match version {
        ImporterDepVersion::Regular(version) => *version = moved.suffix.clone(),
        ImporterDepVersion::Alias(key) => *key = moved.clone(),
        ImporterDepVersion::File(payload) => {
            *payload = moved.suffix
                .to_string()
                .strip_prefix("file:")
                .expect("rebuilding peer keys preserves the file protocol")
                .to_string();
        }
        ImporterDepVersion::Link(_) => {}
    }
}

#[cfg(test)]
mod tests;

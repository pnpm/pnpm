//! Fold one lockfile's changes into another — how an install under
//! `mergeGitBranchLockfiles` combines the per-branch lockfiles with the
//! shared `pnpm-lock.yaml`.
//!
//! The result carries only what pnpm's `mergeLockfileChanges` keeps: the
//! importers, the packages, the lockfile version, the pnpmfile checksum,
//! and the ignored optional dependencies. Settings, catalogs, overrides,
//! and the other recorded-config fields are dropped; the install that
//! consumes the merge writes its own back.

use crate::{
    Lockfile, LockfileExtra, LockfileVersion, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use node_semver::Version;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

/// Merge `theirs` into `ours`, preferring the higher version wherever the
/// two disagree on how a dependency resolved.
#[must_use]
pub fn merge_lockfile_changes(ours: &Lockfile, theirs: &Lockfile) -> Lockfile {
    Lockfile {
        lockfile_version: newer_version(ours.lockfile_version, theirs.lockfile_version),
        pnpmfile_checksum: ours
            .pnpmfile_checksum
            .clone()
            .or_else(|| theirs.pnpmfile_checksum.clone()),
        ignored_optional_dependencies: union_of_lists(
            ours.ignored_optional_dependencies.as_deref(),
            theirs.ignored_optional_dependencies.as_deref(),
        ),
        importers: merge_importers(&ours.importers, &theirs.importers),
        packages: merge_maps(ours.packages.as_ref(), theirs.packages.as_ref(), spread),
        snapshots: merge_maps(ours.snapshots.as_ref(), theirs.snapshots.as_ref(), merge_snapshot),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        patched_dependencies: None,
        time: None,
        extra: merge_extra(&ours.extra, &theirs.extra),
    }
}

/// Union the top-level keys pnpm does not define, ours winning a conflict —
/// the same precedence the fields above use. Dropping them would delete the
/// state a tool driving pnpm records beside the lockfile from whichever
/// branch is being merged.
fn merge_extra(ours: &LockfileExtra, theirs: &LockfileExtra) -> LockfileExtra {
    let mut merged = theirs.clone();
    for (key, value) in ours {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

/// Which side of a disagreement a merge keeps.
enum Winner {
    Ours,
    Theirs,
}

/// pnpm's `mergeVersions` reduced to which side wins.
///
/// Both inputs are the rendered `version` of a resolved dependency, so
/// each may carry a peer suffix (`17.0.2(react@17.0.2)`) or not be semver
/// at all (`link:../pkg`). The suffix is not part of the comparison, and
/// anything neither side can parse resolves to theirs — the same "prefer
/// the incoming change" fallback pnpm applies.
fn winner(ours: &str, theirs: &str) -> Winner {
    if ours == theirs {
        return Winner::Ours;
    }
    let without_peers = |version: &str| version.split('(').next().unwrap_or(version).to_owned();
    match (without_peers(ours).parse::<Version>(), without_peers(theirs).parse::<Version>()) {
        (Ok(ours), Ok(theirs)) if ours > theirs => Winner::Ours,
        _ => Winner::Theirs,
    }
}

/// pnpm's `takeChangedValue`: the incoming value, unless it is what we
/// already had.
fn take_changed(ours: &str, theirs: &str) -> String {
    if ours == theirs { ours.to_owned() } else { theirs.to_owned() }
}

fn newer_version(ours: LockfileVersion<9>, theirs: LockfileVersion<9>) -> LockfileVersion<9> {
    let key = |version: LockfileVersion<9>| (version.major, version.minor);
    if key(theirs) > key(ours) { theirs } else { ours }
}

fn union_of_lists(ours: Option<&[String]>, theirs: Option<&[String]>) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    let union: Vec<String> = ours
        .unwrap_or_default()
        .iter()
        .chain(theirs.unwrap_or_default())
        .filter(|entry| seen.insert(entry.as_str()))
        .cloned()
        .collect();
    (!union.is_empty()).then_some(union)
}

/// Merge two optional maps key by key: an entry both sides carry goes
/// through `merge`, an entry only one side carries is taken as is. `None`
/// only when neither side has a map at all.
fn merge_maps<Key: Hash + Eq + Clone, Value: Clone>(
    ours: Option<&HashMap<Key, Value>>,
    theirs: Option<&HashMap<Key, Value>>,
    merge: impl Fn(&Value, &Value) -> Value,
) -> Option<HashMap<Key, Value>> {
    if ours.is_none() && theirs.is_none() {
        return None;
    }
    let mut merged = ours.cloned().unwrap_or_default();
    for (key, their_value) in theirs.cloned().unwrap_or_default() {
        let value = match merged.get(&key) {
            Some(our_value) => merge(our_value, &their_value),
            None => their_value,
        };
        merged.insert(key, value);
    }
    Some(merged)
}

/// Same as [`merge_maps`], but an empty result collapses to `None` — pnpm
/// deletes a dependency group that merged to nothing rather than
/// recording an empty one.
fn merge_dependency_group<Key: Hash + Eq + Clone, Value: Clone>(
    ours: Option<&HashMap<Key, Value>>,
    theirs: Option<&HashMap<Key, Value>>,
    merge: impl Fn(&Value, &Value) -> Value,
) -> Option<HashMap<Key, Value>> {
    merge_maps(ours, theirs, merge).filter(|merged| !merged.is_empty())
}

fn merge_importers(
    ours: &HashMap<String, ProjectSnapshot>,
    theirs: &HashMap<String, ProjectSnapshot>,
) -> HashMap<String, ProjectSnapshot> {
    let mut merged: HashMap<String, ProjectSnapshot> = HashMap::new();
    for id in ours.keys().chain(theirs.keys()) {
        if merged.contains_key(id) {
            continue;
        }
        let (our_importer, their_importer) = (ours.get(id), theirs.get(id));
        let group = |select: fn(&ProjectSnapshot) -> Option<&ResolvedDependencyMap>| {
            merge_dependency_group(
                our_importer.and_then(select),
                their_importer.and_then(select),
                merge_resolved_dependency,
            )
        };
        merged.insert(
            id.clone(),
            ProjectSnapshot {
                specifiers: None,
                dependencies: group(|importer| importer.dependencies.as_ref()),
                dev_dependencies: group(|importer| importer.dev_dependencies.as_ref()),
                optional_dependencies: group(|importer| importer.optional_dependencies.as_ref()),
                dependencies_meta: None,
                publish_directory: None,
            },
        );
    }
    merged
}

/// The v9 importer block inlines the specifier next to the resolved
/// version, where pnpm's in-memory lockfile keeps two parallel maps. Each
/// half still merges under its own rule: the specifier is a plain
/// "changed wins", the version a semver comparison.
fn merge_resolved_dependency(
    ours: &ResolvedDependencySpec,
    theirs: &ResolvedDependencySpec,
) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: take_changed(&ours.specifier, &theirs.specifier),
        version: match winner(&ours.version.to_string(), &theirs.version.to_string()) {
            Winner::Ours => ours.version.clone(),
            Winner::Theirs => theirs.version.clone(),
        },
    }
}

fn merge_snapshot(ours: &SnapshotEntry, theirs: &SnapshotEntry) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: merge_dependency_group(
            ours.dependencies.as_ref(),
            theirs.dependencies.as_ref(),
            merge_snapshot_dep_ref,
        ),
        optional_dependencies: merge_dependency_group(
            ours.optional_dependencies.as_ref(),
            theirs.optional_dependencies.as_ref(),
            merge_snapshot_dep_ref,
        ),
        ..spread(ours, theirs)
    }
}

fn merge_snapshot_dep_ref(ours: &SnapshotDepRef, theirs: &SnapshotDepRef) -> SnapshotDepRef {
    match winner(&ours.to_string(), &theirs.to_string()) {
        Winner::Ours => ours.clone(),
        Winner::Theirs => theirs.clone(),
    }
}

/// JavaScript's `{ ...ours, ...theirs }` for a serde struct: every field
/// `theirs` records wins, and a field only `ours` records survives. pnpm
/// merges its package entries with that spread; a field-by-field port
/// would have to be revisited every time a lockfile entry grows a key.
///
/// Two entries under the same key describe one published version, so they
/// almost always already agree — and a spread of equal entries is one of
/// them. Taking that shortcut keeps the serialization round-trip off the
/// merge of two lockfiles that only differ in a handful of packages.
fn spread<Entry: Serialize + DeserializeOwned + Clone + PartialEq>(
    ours: &Entry,
    theirs: &Entry,
) -> Entry {
    if ours == theirs {
        return ours.clone();
    }
    let mut merged = to_object(ours);
    merged.extend(to_object(theirs));
    serde_json::from_value(serde_json::Value::Object(merged))
        .expect("a spread of two lockfile entries deserializes back")
}

fn to_object<Entry: Serialize>(entry: &Entry) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(entry) {
        Ok(serde_json::Value::Object(fields)) => fields,
        _ => unreachable!("a lockfile entry serializes to a JSON object"),
    }
}

#[cfg(test)]
mod tests;

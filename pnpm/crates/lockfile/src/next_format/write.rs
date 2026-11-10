//! Peer-suffixed sections to the next format.
//!
//! A reference is written without peer groups, unless reading the result back
//! would select another snapshot for it. The writer reads its own output back
//! and writes those references with their peer groups, until nothing changes.

use super::{NextFormatError, NextSections, RestoreContext, RestoredSections, read};
use crate::{Lockfile, PackageKey, PkgName, ProjectSnapshot, SnapshotDepRef, SnapshotEntry};
use rayon::prelude::*;
use snapshots::Snapshots;
use std::collections::{HashMap, HashSet};

mod importers;
mod peers;
mod snapshots;

/// Reading back can need a reference with peer groups only where an earlier
/// round wrote another reference with them, so the rounds are few. This
/// bounds them for a lockfile that cannot be represented.
const MAX_ROUNDS: usize = 8;

/// The next format's sections, read back and checked against the lockfile.
pub(super) fn next_sections(lockfile: &Lockfile) -> Result<NextSections, NextFormatError> {
    let empty = HashMap::new();
    let suffixed = lockfile.snapshots.as_ref().unwrap_or(&empty);
    let snapshots = Snapshots::of(suffixed, lockfile.packages.as_ref())?;
    let context = RestoreContext {
        packages: lockfile.packages.as_ref(),
        settings: lockfile.settings.as_ref(),
        patched_dependencies: lockfile.patched_dependencies.as_ref(),
    };
    let mut explicit = Explicit::default();
    for _ in 0..MAX_ROUNDS {
        let sections = snapshots.sections(lockfile, &explicit)?;
        let Some(rebuilt) = read::Rebuilt::of(&sections, context) else {
            return Ok(sections);
        };
        if !snapshots.mark_mismatches(lockfile, &rebuilt, &mut explicit) {
            // The marks above cover which snapshot each reference selects
            // and the key each snapshot gets. The entries themselves can
            // still read back differently, for a lockfile an older pnpm
            // wrote, so the whole sections are compared once.
            check_round_trip(lockfile, &rebuilt.into_sections(&sections, context))?;
            return Ok(sections);
        }
    }
    Err(NextFormatError::NotRepresentable { detail: "the references did not settle".to_string() })
}

/// The references written with their peer groups.
#[derive(Default)]
pub(super) struct Explicit {
    snapshot_references: HashMap<PackageKey, HashSet<PkgName>>,
    importer_references: HashMap<String, HashSet<PkgName>>,
}

impl Explicit {
    fn has_snapshot_reference(&self, key: &PackageKey, name: &PkgName) -> bool {
        self.snapshot_references
            .get(key)
            .is_some_and(|names| names.contains(name))
    }

    fn has_importer_reference(&self, id: &str, name: &PkgName) -> bool {
        self.importer_references
            .get(id)
            .is_some_and(|names| names.contains(name))
    }

    fn mark_snapshot_reference(&mut self, key: &PackageKey, name: &PkgName) -> bool {
        if self.has_snapshot_reference(key, name) {
            return false;
        }
        self.snapshot_references
            .entry(key.clone())
            .or_default()
            .insert(name.clone())
    }

    fn mark_importer_reference(&mut self, id: &str, name: &PkgName) -> bool {
        if self.has_importer_reference(id, name) {
            return false;
        }
        self.importer_references
            .entry(id.to_string())
            .or_default()
            .insert(name.clone())
    }
}

/// Fails unless `restored` equals the lockfile's own sections, as written:
/// an importer's `specifiers` is the resolver's working copy and never
/// reaches the file, so it does not count.
pub(super) fn check_round_trip(
    lockfile: &Lockfile,
    restored: &RestoredSections,
) -> Result<(), NextFormatError> {
    if let Some((id, importer)) = lockfile.importers
        .iter()
        .find(|(id, importer)| !same_as_written(importer, restored.importers.get(*id)))
    {
        let fields = differing_importer_fields(importer, restored.importers.get(id));
        return Err(NextFormatError::NotRepresentable {
            detail: format!("importer {id}: {fields}"),
        });
    }
    let empty = HashMap::new();
    let original = lockfile.snapshots.as_ref().unwrap_or(&empty);
    let restored = restored.snapshots.as_ref().unwrap_or(&empty);
    if let Some(key) = original
        .par_iter()
        .find_any(|(key, entry)| !restored.get(*key).is_some_and(|other| same_entry(entry, other)))
        .map(|(key, _)| key)
        .or_else(|| {
            restored
                .keys()
                .find(|key| !original.contains_key(*key))
        })
    {
        return Err(NextFormatError::NotRepresentable { detail: format!("snapshot {key}") });
    }
    Ok(())
}

/// Equal, with `transitivePeerDependencies` compared as a set.
fn same_entry(left: &SnapshotEntry, right: &SnapshotEntry) -> bool {
    let sorted = |entry: &SnapshotEntry| {
        let mut names = entry.transitive_peer_dependencies.clone().unwrap_or_default();
        names.sort();
        names
    };
    left.id == right.id
        && left.dependencies == right.dependencies
        && left.optional_dependencies == right.optional_dependencies
        && left.patched == right.patched
        && left.optional == right.optional
        && sorted(left) == sorted(right)
}

pub(super) fn children(entry: &SnapshotEntry) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    entry.dependencies
        .iter()
        .chain(entry.optional_dependencies.iter())
        .flatten()
}

/// Whether `restored` is `original` as the file holds it.
fn same_as_written(original: &ProjectSnapshot, restored: Option<&ProjectSnapshot>) -> bool {
    restored.is_some_and(|restored| {
        let written = ProjectSnapshot { specifiers: None, ..original.clone() };
        written == *restored
    })
}

/// The names of the importer fields `restored` differs from `original`
/// in, so the error says what did not round-trip.
fn differing_importer_fields(
    original: &ProjectSnapshot,
    restored: Option<&ProjectSnapshot>,
) -> String {
    let Some(restored) = restored else { return "missing".to_string() };
    let differing: Vec<&str> = [
        ("dependencies", original.dependencies != restored.dependencies),
        ("devDependencies", original.dev_dependencies != restored.dev_dependencies),
        ("optionalDependencies", original.optional_dependencies != restored.optional_dependencies),
        ("dependenciesMeta", original.dependencies_meta != restored.dependencies_meta),
        ("publishDirectory", original.publish_directory != restored.publish_directory),
        ("linkDirectory", original.link_directory != restored.link_directory),
    ]
    .into_iter()
    .filter_map(|(name, differs)| differs.then_some(name))
    .collect();
    format!("{} differ", differing.join(", "))
}

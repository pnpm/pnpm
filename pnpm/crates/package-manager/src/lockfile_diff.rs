//! Structural diff between two lockfiles, plus the report
//! `pacquet install --dry-run` prints.
//!
//! `install --dry-run` compares the freshly-resolved lockfile against the
//! existing on-disk one to preview what a real install would change;
//! `dedupe --check` compares the pre- and post-deduplication lockfiles to
//! report what deduplication would rewrite. Mirrors pnpm's
//! `calcDedupeCheckIssues`, which serves both commands the same way.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use pnpm_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotDepRef, SnapshotEntry};

/// What a real install would change, derived from two lockfiles.
///
/// Package-level changes are diffed over the v9 `snapshots:` map — the
/// peer-aware dependency wiring a real install rewrites — to match pnpm's
/// `dedupeDiffCheck`, whose in-memory `packages` map is depPath-keyed.
#[derive(Debug, Default)]
pub struct LockfileDiff {
    /// Per-importer direct-dependency changes, in importer-id order.
    pub importers: Vec<SnapshotDiff>,
    /// `snapshots:` keys present in the new lockfile but not the old.
    pub added_packages: Vec<String>,
    /// `snapshots:` keys present in the old lockfile but not the new.
    pub removed_packages: Vec<String>,
    /// `snapshots:` entries present in both whose dependency wiring changed.
    pub updated_packages: Vec<SnapshotDiff>,
}

/// Dependency changes within a single snapshot — an importer's direct
/// dependencies or one `snapshots:` entry's dependency wiring.
#[derive(Debug)]
pub struct SnapshotDiff {
    /// The importer id or `snapshots:` key these changes belong to.
    pub id: String,
    /// `(alias, value)` pairs newly added.
    pub added: Vec<(String, String)>,
    /// `(alias, value)` pairs removed.
    pub removed: Vec<(String, String)>,
    /// `(alias, old_value, new_value)` pairs whose value changed.
    pub updated: Vec<(String, String, String)>,
}

/// Which value an importer's direct dependencies are compared by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImporterDiffKey {
    /// The manifest specifier. A real install rewrites the lockfile
    /// whenever a specifier changes — even when it still resolves to the
    /// same version — so `install --dry-run` previews by specifier.
    Specifier,
    /// The resolved version. Deduplication rewrites peer-resolved
    /// versions while the specifiers stay put, so `dedupe --check`
    /// compares by version, like pnpm's, which diffs the importers'
    /// resolved-dependency fields and leaves their specifiers out.
    Version,
}

impl SnapshotDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

/// What happened to one alias between the two lockfiles.
enum AliasChange {
    Added(String),
    Removed(String),
    Updated { prev: String, next: String },
}

/// The per-alias changes of one snapshot, merged across its dependency
/// groups.
///
/// pnpm folds every group's diff into one alias-keyed map, so an alias
/// that only moves between groups — `devDependencies` to `dependencies`,
/// say — yields the last group's single verdict rather than an addition
/// and a removal that contradict each other. Merging in pnpm's group
/// order therefore decides which verdict wins.
#[derive(Default)]
struct AliasChanges(BTreeMap<String, AliasChange>);

impl AliasChanges {
    /// Record the per-alias differences between two `alias -> value` maps.
    /// Mirrors pnpm's `getResolutionUpdates`.
    fn merge(&mut self, old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) {
        for (alias, new_value) in new {
            match old.get(alias) {
                None => {
                    self.0.insert(alias.clone(), AliasChange::Added(new_value.clone()));
                }
                Some(old_value) if old_value != new_value => {
                    let change =
                        AliasChange::Updated { prev: old_value.clone(), next: new_value.clone() };
                    self.0.insert(alias.clone(), change);
                }
                Some(_) => {}
            }
        }
        for (alias, old_value) in old {
            if !new.contains_key(alias) {
                self.0.insert(alias.clone(), AliasChange::Removed(old_value.clone()));
            }
        }
    }

    fn into_diff(self, id: String) -> SnapshotDiff {
        let mut diff =
            SnapshotDiff { id, added: Vec::new(), removed: Vec::new(), updated: Vec::new() };
        for (alias, change) in self.0 {
            match change {
                AliasChange::Added(next) => diff.added.push((alias, next)),
                AliasChange::Removed(prev) => diff.removed.push((alias, prev)),
                AliasChange::Updated { prev, next } => diff.updated.push((alias, prev, next)),
            }
        }
        diff
    }
}

impl LockfileDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.importers.is_empty()
            && self.added_packages.is_empty()
            && self.removed_packages.is_empty()
            && self.updated_packages.is_empty()
    }
}

/// Diff the existing lockfile (`old`) against the freshly-resolved one
/// (`new`). A `None` `new` yields an empty diff — there is nothing a real
/// install would produce to compare against.
#[must_use]
pub fn diff_lockfiles(
    old: Option<&Lockfile>,
    new: Option<&Lockfile>,
    importer_key: ImporterDiffKey,
) -> LockfileDiff {
    let Some(new) = new else {
        return LockfileDiff::default();
    };

    let mut diff = LockfileDiff::default();

    let mut importer_ids: BTreeSet<&str> = new.importers.keys().map(String::as_str).collect();
    if let Some(old) = old {
        importer_ids.extend(old.importers.keys().map(String::as_str));
    }
    for id in importer_ids {
        let importer_diff = diff_importer(
            id,
            old.and_then(|lockfile| lockfile.importers.get(id)),
            new.importers.get(id),
            importer_key,
        );
        if !importer_diff.is_empty() {
            diff.importers.push(importer_diff);
        }
    }

    diff_snapshots(old, Some(new), &mut diff);

    diff
}

/// Diff the v9 `snapshots:` map — the peer-aware dependency wiring a real
/// install rewrites — by key set and by `dependencies` /
/// `optionalDependencies`. Mirrors pnpm's `dedupeDiffCheck`, which diffs its
/// depPath-keyed `packages` snapshots the same way. Results are sorted.
fn diff_snapshots(old: Option<&Lockfile>, new: Option<&Lockfile>, diff: &mut LockfileDiff) {
    let old_snapshots = old.and_then(|lockfile| lockfile.snapshots.as_ref());
    let new_snapshots = new.and_then(|lockfile| lockfile.snapshots.as_ref());

    for (key, new_entry) in new_snapshots.into_iter().flatten() {
        match old_snapshots.and_then(|snapshots| snapshots.get(key)) {
            None => diff.added_packages.push(key.to_string()),
            // The equality check keeps the common case — a snapshot both
            // lockfiles wire identically — off the per-alias path, which
            // allocates a map per dependency group on both sides.
            Some(old_entry) if snapshot_wiring_differs(old_entry, new_entry) => {
                let entry_diff = diff_snapshot_entry(key.to_string(), old_entry, new_entry);
                if !entry_diff.is_empty() {
                    diff.updated_packages.push(entry_diff);
                }
            }
            Some(_) => {}
        }
    }
    for key in old_snapshots.into_iter().flatten().map(|(key, _)| key) {
        if new_snapshots.is_none_or(|snapshots| !snapshots.contains_key(key)) {
            diff.removed_packages.push(key.to_string());
        }
    }

    diff.added_packages.sort();
    diff.removed_packages.sort();
    diff.updated_packages.sort_by(|left, right| left.id.cmp(&right.id));
}

/// Whether a real install would rewrite this snapshot's dependency wiring.
/// Compares only `dependencies` / `optionalDependencies`, matching pnpm's
/// `PACKAGE_SNAPSHOT_DEP_FIELDS`.
fn snapshot_wiring_differs(old: &SnapshotEntry, new: &SnapshotEntry) -> bool {
    old.dependencies != new.dependencies || old.optional_dependencies != new.optional_dependencies
}

/// Diff one `snapshots:` entry's dependency wiring, over `dependencies` /
/// `optionalDependencies` only — pnpm's `PACKAGE_SNAPSHOT_DEP_FIELDS`.
fn diff_snapshot_entry(key: String, old: &SnapshotEntry, new: &SnapshotEntry) -> SnapshotDiff {
    let mut changes = AliasChanges::default();
    changes.merge(&dep_refs(old.dependencies.as_ref()), &dep_refs(new.dependencies.as_ref()));
    changes.merge(
        &dep_refs(old.optional_dependencies.as_ref()),
        &dep_refs(new.optional_dependencies.as_ref()),
    );
    changes.into_diff(key)
}

/// One dependency group of a `snapshots:` entry as an
/// `alias -> resolved reference` map.
fn dep_refs(deps: Option<&HashMap<PkgName, SnapshotDepRef>>) -> BTreeMap<String, String> {
    deps.into_iter()
        .flatten()
        .map(|(name, dep_ref)| (name.to_string(), dep_ref.to_string()))
        .collect()
}

fn diff_importer(
    id: &str,
    old: Option<&ProjectSnapshot>,
    new: Option<&ProjectSnapshot>,
    key: ImporterDiffKey,
) -> SnapshotDiff {
    let mut changes = AliasChanges::default();
    for group in IMPORTER_GROUPS {
        changes.merge(&group_deps(old, group, key), &group_deps(new, group, key));
    }
    changes.into_diff(id.to_string())
}

/// The importer dependency groups, in pnpm's `DEPENDENCIES_FIELDS` order —
/// which decides the winner for an alias that moves between them.
const IMPORTER_GROUPS: [ImporterGroup; 3] =
    [ImporterGroup::Optional, ImporterGroup::Prod, ImporterGroup::Dev];

#[derive(Debug, Clone, Copy)]
enum ImporterGroup {
    Prod,
    Dev,
    Optional,
}

/// The `alias -> specifier` (or `alias -> version`) map for one dependency
/// group of an importer.
fn group_deps(
    snapshot: Option<&ProjectSnapshot>,
    group: ImporterGroup,
    key: ImporterDiffKey,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(snapshot) = snapshot else {
        return map;
    };
    let deps = match group {
        ImporterGroup::Prod => &snapshot.dependencies,
        ImporterGroup::Dev => &snapshot.dev_dependencies,
        ImporterGroup::Optional => &snapshot.optional_dependencies,
    };
    if let Some(deps) = deps {
        for (name, spec) in deps {
            let value = match key {
                ImporterDiffKey::Specifier => spec.specifier.clone(),
                ImporterDiffKey::Version => spec.version.to_string(),
            };
            map.insert(name.to_string(), value);
        }
    }
    map
}

/// Render a [`LockfileDiff`] into the report `pacquet install --dry-run`
/// prints to stdout.
#[must_use]
pub fn render_dry_run_report(diff: &LockfileDiff) -> String {
    if diff.is_empty() {
        return "Dry run complete. pnpm-lock.yaml is up to date; a real install would make no changes."
            .to_string();
    }

    let mut lines = vec![
        "Dry run complete. A real install would make the following changes (nothing was written to disk):"
            .to_string(),
        String::new(),
    ];

    if !diff.importers.is_empty() {
        lines.push("Importers".to_string());
        for importer in &diff.importers {
            lines.push(importer.id.clone());
            for (alias, version) in &importer.added {
                lines.push(format!("  + {alias} {version}"));
            }
            for (alias, version) in &importer.removed {
                lines.push(format!("  - {alias} {version}"));
            }
            for (alias, old, new) in &importer.updated {
                lines.push(format!("  {alias} {old} -> {new}"));
            }
        }
        lines.push(String::new());
    }

    if !diff.added_packages.is_empty()
        || !diff.removed_packages.is_empty()
        || !diff.updated_packages.is_empty()
    {
        lines.push("Packages".to_string());
        for key in &diff.added_packages {
            lines.push(format!("+ {key}"));
        }
        for key in &diff.removed_packages {
            lines.push(format!("- {key}"));
        }
        for package in &diff.updated_packages {
            lines.push(format!("~ {}", package.id));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests;

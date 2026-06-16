//! Diff + report for `pacquet install --dry-run`.
//!
//! Compares the freshly-resolved lockfile against the existing on-disk one
//! and renders a human report of what a real install would change, without
//! writing anything. Mirrors pnpm's `install --dry-run` preview.

use std::collections::{BTreeMap, BTreeSet};

use pacquet_lockfile::{Lockfile, ProjectSnapshot, SnapshotEntry};

/// What a real install would change, derived from two lockfiles.
///
/// Package-level changes are diffed over the v9 `snapshots:` map — the
/// peer-aware dependency wiring a real install rewrites — to match pnpm's
/// `dedupeDiffCheck`, whose in-memory `packages` map is depPath-keyed.
#[derive(Debug, Default)]
pub struct LockfileDiff {
    /// Per-importer direct-dependency changes, in importer-id order.
    pub importers: Vec<ImporterDiff>,
    /// `snapshots:` keys present in the new lockfile but not the old.
    pub added_packages: Vec<String>,
    /// `snapshots:` keys present in the old lockfile but not the new.
    pub removed_packages: Vec<String>,
    /// `snapshots:` keys present in both whose dependency wiring changed.
    pub updated_packages: Vec<String>,
}

/// Direct-dependency changes for a single importer.
#[derive(Debug)]
pub struct ImporterDiff {
    pub id: String,
    /// `(alias, version)` pairs newly added.
    pub added: Vec<(String, String)>,
    /// `(alias, version)` pairs removed.
    pub removed: Vec<(String, String)>,
    /// `(alias, old_version, new_version)` pairs whose resolution changed.
    pub updated: Vec<(String, String, String)>,
}

impl ImporterDiff {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
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
pub fn diff_lockfiles(old: Option<&Lockfile>, new: Option<&Lockfile>) -> LockfileDiff {
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
            Some(old_entry) if snapshot_wiring_differs(old_entry, new_entry) => {
                diff.updated_packages.push(key.to_string());
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
    diff.updated_packages.sort();
}

/// Whether a real install would rewrite this snapshot's dependency wiring.
/// Compares only `dependencies` / `optionalDependencies`, matching pnpm's
/// `PACKAGE_SNAPSHOT_DEP_FIELDS`.
fn snapshot_wiring_differs(old: &SnapshotEntry, new: &SnapshotEntry) -> bool {
    old.dependencies != new.dependencies || old.optional_dependencies != new.optional_dependencies
}

fn diff_importer(
    id: &str,
    old: Option<&ProjectSnapshot>,
    new: Option<&ProjectSnapshot>,
) -> ImporterDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut updated = Vec::new();

    // Diff each dependency group independently so a dependency that moves
    // between groups (e.g. dev -> prod) registers as a change. Mirrors
    // pnpm's `dedupeDiffCheck`, which diffs `dependencies`,
    // `devDependencies`, and `optionalDependencies` separately. The
    // `specifier` field is intentionally not compared: pnpm's diff ignores
    // it too (specifiers live in a separate map outside its diff fields).
    for group in 0..3 {
        let old_deps = group_versions(old, group);
        let new_deps = group_versions(new, group);
        for (alias, new_version) in &new_deps {
            match old_deps.get(alias) {
                None => added.push((alias.clone(), new_version.clone())),
                Some(old_version) if old_version != new_version => {
                    updated.push((alias.clone(), old_version.clone(), new_version.clone()));
                }
                Some(_) => {}
            }
        }
        for (alias, old_version) in &old_deps {
            if !new_deps.contains_key(alias) {
                removed.push((alias.clone(), old_version.clone()));
            }
        }
    }

    ImporterDiff { id: id.to_string(), added, removed, updated }
}

/// The `alias -> resolved version` map for one dependency group of an
/// importer (0 = prod, 1 = dev, 2 = optional).
fn group_versions(snapshot: Option<&ProjectSnapshot>, group: usize) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(snapshot) = snapshot else {
        return map;
    };
    let deps = match group {
        0 => &snapshot.dependencies,
        1 => &snapshot.dev_dependencies,
        _ => &snapshot.optional_dependencies,
    };
    if let Some(deps) = deps {
        for (name, spec) in deps {
            map.insert(name.to_string(), spec.version.to_string());
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
        for key in &diff.updated_packages {
            lines.push(format!("~ {key}"));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests;

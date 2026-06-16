//! Diff + report for `pacquet install --dry-run`.
//!
//! Compares the freshly-resolved lockfile against the existing on-disk one
//! and renders a human report of what a real install would change, without
//! writing anything. Mirrors pnpm's `install --dry-run` preview.

use std::collections::{BTreeMap, BTreeSet};

use pacquet_lockfile::{Lockfile, ProjectSnapshot};

/// What a real install would change, derived from two lockfiles.
#[derive(Debug, Default)]
pub struct LockfileDiff {
    /// Per-importer direct-dependency changes, in importer-id order.
    pub importers: Vec<ImporterDiff>,
    /// `packages:` keys present in the new lockfile but not the old.
    pub added_packages: Vec<String>,
    /// `packages:` keys present in the old lockfile but not the new.
    pub removed_packages: Vec<String>,
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

    let new_keys = package_keys(Some(new));
    let old_keys = package_keys(old);
    diff.added_packages = new_keys.difference(&old_keys).cloned().collect();
    diff.removed_packages = old_keys.difference(&new_keys).cloned().collect();

    diff
}

fn package_keys(lockfile: Option<&Lockfile>) -> BTreeSet<String> {
    lockfile
        .and_then(|lockfile| lockfile.packages.as_ref())
        .map(|packages| packages.keys().map(ToString::to_string).collect())
        .unwrap_or_default()
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

    if !diff.added_packages.is_empty() || !diff.removed_packages.is_empty() {
        lines.push("Packages".to_string());
        for key in &diff.added_packages {
            lines.push(format!("+ {key}"));
        }
        for key in &diff.removed_packages {
            lines.push(format!("- {key}"));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests;

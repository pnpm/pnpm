//! Targeted cleanup for an `included` (dependency-group selection)
//! drift: remove the direct-dependency links a previous install created
//! for groups the current install excludes, leaving everything else in
//! the importer's `node_modules` — the user's own files above all — in
//! place. The non-destructive counterpart of the purge in
//! [`crate::Install`], mirroring pnpm's `removeDirectDependency` prune.

use crate::{
    SkippedSnapshots,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
    symlink_direct_dependencies::{
        direct_dep_names_for_importer, importer_root_dir, validate_importer_id,
    },
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::{Host, get_bins_from_package_manifest, remove_bin};
use pacquet_config::Config;
use pacquet_fs::{read_symlink_dir, remove_symlink_dir};
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::DependencyGroup;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

/// Error type of [`prune_direct_deps_excluded_by_groups`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PruneDirectDepsError {
    /// A direct-dep alias recorded in the current lockfile is not a
    /// valid npm package name. Joining it under `node_modules` could
    /// escape the directory, so the removal is refused — same boundary
    /// `safe_join_modules_dir` enforces on the hoisted restore path.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),

    #[display(
        "Failed to read {path:?} while removing the bins of an excluded direct dependency: {error}"
    )]
    #[diagnostic(code(pacquet_package_manager::prune_direct_deps_read_manifest))]
    ReadManifest {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display(
        "Failed to remove the bin shim at {path:?} of an excluded direct dependency: {error}"
    )]
    #[diagnostic(code(pacquet_package_manager::prune_direct_deps_remove_bin))]
    RemoveBin {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove the excluded direct dependency at {path:?}: {error}")]
    #[diagnostic(code(pacquet_package_manager::prune_direct_deps_remove_link))]
    RemoveLink {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Remove the direct-dependency links that `old_included` selected but
/// `new_included` does not, for every importer recorded in the current
/// lockfile (`<virtual_store_dir>/lock.yaml` — what the previous install
/// actually materialized).
///
/// Runs when `.modules.yaml` records a different `included` set than the
/// current install wants while the layout itself is unchanged: that
/// drift skips the destructive purge, so the links the previous install
/// created for now-excluded groups have to be removed individually —
/// pnpm does the same through its prune's `removeDirectDependency`.
/// Removal is conservative on three axes:
///
/// - Each importer's modules dir must canonicalize to a path inside the
///   workspace root (`confined_modules_dir`, the purge's containment
///   check), and removals never reach through an intermediate
///   `@scope/` or `.bin/` component that isn't a real directory — a
///   symlinked or junctioned one could redirect the deletions outside
///   the confined dir.
/// - Only symlinks (junctions on Windows) are removed. The isolated
///   linker only ever writes links for direct deps, so a real file or
///   directory under the same name belongs to the user — or to the
///   hoisted linker, whose own orphan diff handles removal.
/// - A dep's bin shims are removed first, resolved from the same
///   sanitized [`get_bins_from_package_manifest`] name set the shim
///   writer uses, so removal touches exactly the files the writer could
///   have created. The relink that follows this cleanup re-creates any
///   shim a still-included dep owns.
///
/// Over-removal is self-healing: anything in the new `included` set is
/// re-linked right after by [`crate::SymlinkDirectDependencies`], so the
/// diff only has to be correct for entries the new install won't touch.
pub fn prune_direct_deps_excluded_by_groups(
    current_lockfile: &Lockfile,
    old_included: IncludedDependencies,
    new_included: IncludedDependencies,
    workspace_root: &Path,
    config: &Config,
) -> Result<(), PruneDirectDepsError> {
    let old_groups = selected_groups(old_included);
    let new_groups = selected_groups(new_included);
    // The skip filter only narrows what the linker materializes. For
    // removal, a name that was skipped last install has no on-disk
    // entry, so removing it is a no-op — no skip set needed.
    let skipped = SkippedSnapshots::new();
    // Same per-importer `modulesDir` suffix peeling as
    // [`crate::SymlinkDirectDependencies`], so removal targets exactly
    // where the linker writes.
    let modules_dir_name: &OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));

    for (importer_id, snapshot) in &current_lockfile.importers {
        // A malformed importer key is rejected with a typed error by
        // the symlink pass; never *delete* based on one.
        if validate_importer_id(importer_id).is_err() {
            continue;
        }
        // Same canonical containment check as the purge: delete only
        // through a modules dir that resolves inside the workspace
        // root, and operate on the canonicalized path so a symlink
        // swap can't redirect the removals after the check.
        let modules_dir = importer_root_dir(workspace_root, importer_id).join(modules_dir_name);
        let Some(modules_dir) = confined_modules_dir(&modules_dir, workspace_root) else {
            continue;
        };
        let new_names: HashSet<String> =
            direct_dep_names_for_importer(snapshot, new_groups.iter().copied(), &skipped, false)
                .into_iter()
                .collect();
        for name in
            direct_dep_names_for_importer(snapshot, old_groups.iter().copied(), &skipped, false)
        {
            if new_names.contains(&name) {
                continue;
            }
            remove_direct_dep_link(&modules_dir, &name)?;
        }
    }
    Ok(())
}

/// Canonicalize `modules_dir` and require it to stay within
/// `workspace_root` — the same containment check the purge applies
/// before its destructive sweep. Returns the canonical path the
/// removals should operate on, or `None` when there is nothing to
/// prune (the directory doesn't exist) or when the resolution escapes
/// the workspace — never delete through an escape.
fn confined_modules_dir(modules_dir: &Path, workspace_root: &Path) -> Option<PathBuf> {
    let modules_canon = std::fs::canonicalize(modules_dir).ok()?;
    let root_canon = std::fs::canonicalize(workspace_root).ok()?;
    if modules_canon.starts_with(&root_canon) {
        Some(modules_canon)
    } else {
        tracing::warn!(
            ?modules_dir,
            "refusing to prune direct dependencies outside the workspace root",
        );
        None
    }
}

/// `true` when `path` is a real directory — not a symlink (nor a
/// junction on Windows, which `FileType::is_dir` reports as a
/// directory). Removals must not reach *through* a redirected
/// intermediate component (`@scope/`, `.bin/`); the containment check
/// above only vouches for the modules dir itself.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
        && read_symlink_dir(path).is_err()
}

fn selected_groups(included: IncludedDependencies) -> Vec<DependencyGroup> {
    let mut groups = Vec::with_capacity(3);
    if included.dependencies {
        groups.push(DependencyGroup::Prod);
    }
    if included.dev_dependencies {
        groups.push(DependencyGroup::Dev);
    }
    if included.optional_dependencies {
        groups.push(DependencyGroup::Optional);
    }
    groups
}

fn remove_direct_dep_link(modules_dir: &Path, name: &str) -> Result<(), PruneDirectDepsError> {
    let link =
        safe_join_modules_dir(modules_dir, name).map_err(PruneDirectDepsError::InvalidAlias)?;
    // For a scoped alias the join passes through `@scope/`; refuse to
    // unlink through one that is not a real directory.
    if let Some(parent) = link.parent()
        && parent != modules_dir
        && !is_real_dir(parent)
    {
        return Ok(());
    }
    // Only a symlink (junction on Windows) can be a direct-dep entry the
    // isolated linker wrote; anything else stays untouched. The probe
    // also covers "not there at all" — nothing to remove.
    if read_symlink_dir(&link).is_err() {
        return Ok(());
    }
    remove_dep_bins(modules_dir, &link)?;
    match remove_symlink_dir(&link) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PruneDirectDepsError::RemoveLink { path: link, error }),
    }
}

/// Remove the shims the package behind `link` declares from
/// `<modules_dir>/.bin`. A missing or unparsable `package.json` (a
/// dangling link, a broken package) yields no bins to remove — the same
/// best-effort read as pnpm's `removeBins`.
fn remove_dep_bins(modules_dir: &Path, link: &Path) -> Result<(), PruneDirectDepsError> {
    let manifest_path = link.join("package.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(PruneDirectDepsError::ReadManifest { path: manifest_path, error });
        }
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(());
    };
    // Shims are deleted with plain `remove_file`, so a `.bin` that is
    // itself a symlink or junction would redirect the deletions
    // outside the confined modules dir — skip shim removal unless
    // `.bin` is a real directory. (Absent `.bin` means no shims.)
    let bins_dir = modules_dir.join(".bin");
    if !is_real_dir(&bins_dir) {
        return Ok(());
    }
    for command in get_bins_from_package_manifest::<Host>(&manifest, link) {
        let shim_path = bins_dir.join(&command.name);
        remove_bin(&shim_path)
            .map_err(|error| PruneDirectDepsError::RemoveBin { path: shim_path, error })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

//! Removing the links that point at a skipped optional dependency.

use super::{BuildModulesError, BuildOneSnapshot, PackageKey};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The directories of a skipped package, resolved once so each candidate
/// link is compared by its own resolved target, the way pnpm's `linksTo`
/// compares `realpath`s. Resolving needs the directories to exist, so
/// these are built before the package is removed.
pub(super) struct LinkTargets(Vec<PathBuf>);

impl LinkTargets {
    pub(super) fn resolve(dirs: &[PathBuf]) -> Self {
        LinkTargets(
            dirs.iter()
                .filter_map(|dir| std::fs::canonicalize(dir).ok())
                .collect(),
        )
    }

    /// Whether `entry` is a symlink or junction resolving to one of the
    /// targets. `canonicalize` follows junctions as well as symlinks, which
    /// `std::fs::read_link` does not. A link that resolves to nothing points
    /// at none of them.
    fn is_linked_from(&self, entry: &Path) -> bool {
        pnpm_fs::is_symlink_or_junction(entry).unwrap_or(false)
            && std::fs::canonicalize(entry).is_ok_and(|resolved| self.0.contains(&resolved))
    }
}

/// A link that could not be removed, with the path it sits at.
#[derive(Debug)]
pub(super) struct UnlinkError {
    pub(super) path: PathBuf,
    pub(super) source: io::Error,
}

/// Remove the links to `targets` from every project's modules directory and
/// from the hidden hoisting directory, so a skipped optional dependency is
/// absent rather than linked to nothing.
pub(super) fn unlink_project_links(
    context: &BuildOneSnapshot<'_>,
    targets: &LinkTargets,
) -> Result<(), UnlinkError> {
    let install_state_dir =
        context.scripts.patched_engines.install_state_dir.unwrap_or_else(|| {
            context.directories.layout.package_store_dir()
        });
    project_modules_dirs(context)
        .into_iter()
        .chain([install_state_dir.join("node_modules")])
        .try_for_each(|modules_dir| unlink_children(&modules_dir, targets))
}

/// Remove an optional dependency whose build failed together with the links
/// to it. The links are matched by their resolved targets, so they go first,
/// while the package directory still exists.
pub(super) fn discard_failed_optional(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
) -> Result<(), BuildModulesError> {
    let targets = LinkTargets::resolve(&context.pkg_roots().all(snapshot_key));
    unlink_project_links(context, &targets)
        .map_err(|UnlinkError { path, source }| {
            BuildModulesError::RemoveSkippedOptionalDependency { path, source }
        })?;
    super::discard_skipped_optional_dependency(
        context.pkg_roots(),
        context.directories.lockfile_dir,
        snapshot_key,
    )
}

/// The `node_modules` directory of every project in the lockfile. An importer
/// key that would escape the lockfile directory is left out.
fn project_modules_dirs(context: &BuildOneSnapshot<'_>) -> Vec<PathBuf> {
    let root = context.directories.modules_dir;
    let lockfile_dir = context.directories.lockfile_dir;
    let mut dirs = vec![root.to_path_buf()];
    let Ok(modules_dir_name) = root.strip_prefix(lockfile_dir) else { return dirs };
    dirs.extend(
        context.graph.importers
            .keys()
            .filter(|importer_id| importer_id.as_str() != ".")
            .filter(|importer_id| crate::validate_importer_id(importer_id).is_ok())
            .map(|importer_id| lockfile_dir.join(importer_id).join(modules_dir_name)),
    );
    dirs
}

/// Remove the entries of `dir`, and of its scope directories, that link to
/// `targets`.
pub(super) fn unlink_children(dir: &Path, targets: &LinkTargets) -> Result<(), UnlinkError> {
    modules_dir_entries(dir)
        .into_iter()
        .filter(|entry| targets.is_linked_from(entry))
        .try_for_each(remove_link)
}

/// The entries of `dir` and of its real scope directories, the places a
/// package can be linked from.
fn modules_dir_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries = list_dir(dir);
    let scoped: Vec<PathBuf> = entries
        .iter()
        .filter(|entry| is_scope_dir(entry))
        .flat_map(|scope| list_dir(scope))
        .collect();
    entries.extend(scoped);
    entries
}

fn list_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map_or_else(
            |_| Vec::new(),
            |entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .collect()
            },
        )
}

/// A real `@scope` directory. A scope directory that is itself a link is
/// not followed.
fn is_scope_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('@'))
        && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
        && pnpm_fs::is_symlink_or_junction(path).is_ok_and(|linked| !linked)
}

fn remove_link(path: PathBuf) -> Result<(), UnlinkError> {
    match pnpm_fs::remove_symlink_dir(&path) {
        Err(source) if source.kind() != io::ErrorKind::NotFound => {
            Err(UnlinkError { path, source })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;

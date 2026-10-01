//! Removing the links a discarded optional dependency leaves behind.

use super::{BuildModulesError, BuildOneSnapshot, PackageKey};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Remove the links to `snapshot_key`'s package that point at nothing once
/// its directory is discarded: each project's direct-dependency link to it
/// and its entry in the hidden hoisting directory. The dependency is then
/// absent, which the repeat-install check accepts, rather than linked to
/// nothing, which it repairs with a full install that reruns the failing
/// build ([#16468](https://github.com/pnpm/pnpm/issues/16468)).
///
/// Only an entry that is a link to a missing target is removed, so an entry
/// another package owns is left alone.
pub(super) fn remove_dangling_links(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    name: &str,
) -> Result<(), BuildModulesError> {
    let hoisted = context.scripts.patched_engines.install_state_dir
        .unwrap_or_else(|| context.directories.layout.package_store_dir())
        .join("node_modules")
        .join(name);
    direct_links(context, snapshot_key)
        .into_iter()
        .chain([hoisted])
        .try_for_each(|link| remove_if_dangling(&link))
}

/// Where each project links `snapshot_key` as a direct dependency. An
/// importer key that would escape the lockfile directory is left out.
fn direct_links(context: &BuildOneSnapshot<'_>, snapshot_key: &PackageKey) -> Vec<PathBuf> {
    let root = context.directories.modules_dir;
    let lockfile_dir = context.directories.lockfile_dir;
    let modules_dir_name = root.strip_prefix(lockfile_dir).ok();
    let mut links = Vec::new();
    for (importer_id, importer) in context.graph.importers {
        let modules_dir = if importer_id == "." {
            root.to_path_buf()
        } else if let (Ok(()), Some(name)) =
            (crate::validate_importer_id(importer_id), modules_dir_name)
        {
            lockfile_dir.join(importer_id).join(name)
        } else {
            continue;
        };
        let groups = [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ];
        links.extend(
            groups
                .into_iter()
                .flatten()
                .flatten()
                .filter(|(alias, spec)| {
                    spec.version.resolved_key(alias).as_ref() == Some(snapshot_key)
                })
                .map(|(alias, _)| modules_dir.join(alias.to_string())),
        );
    }
    links
}

fn remove_if_dangling(link: &Path) -> Result<(), BuildModulesError> {
    let removal_error = |source: io::Error| BuildModulesError::RemoveSkippedOptionalDependency {
        path: link.to_path_buf(),
        source,
    };
    match pnpm_fs::is_symlink_or_junction(link) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(removal_error(error)),
    }
    if std::fs::metadata(link).is_ok() {
        return Ok(());
    }
    match pnpm_fs::remove_symlink_dir(link) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(removal_error(error)),
        _ => Ok(()),
    }
}

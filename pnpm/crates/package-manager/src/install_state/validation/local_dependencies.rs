//! The local dependencies a recorded lockfile resolves. A tarball replaced in
//! place, or a directory edited, changes what an install produces with no
//! manifest or lockfile moving. A `readPackage` hook can add such a
//! dependency to any package, so the check reads them from the record, not
//! from the manifests.

use super::{FileDigest, FileDigests, FileSignature, Path, PathBuf, Validation};
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_workspace_state::LocalDependencies;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Record the local dependencies of the lockfile at `lockfile_path`, and carry
/// the entries of the other lockfiles `carried` records projects against.
pub(super) fn record_for_write(
    validation: &mut Validation,
    inputs: &super::RecordValidation<'_>,
    carried: Option<&Validation>,
    (digest, lockfile_path): (Option<&str>, &Path),
) {
    let current = digest.and_then(|digest| {
        let local =
            of_lockfile(carried, (digest, lockfile_path, inputs.lockfile), inputs.workspace_root)?;
        Some((digest, local))
    });
    let digests = FileDigests::new(carried);
    record(validation, current, (carried, &digests), |signature| {
        super::recordable_signature(inputs.config, signature)
    });
}

/// The local dependencies of the lockfile at `lockfile_path`, whose digest is
/// `digest`: the entry `previous` records for that digest, or the ones
/// `parsed` resolves, or the ones the file resolves. `None` when the file
/// cannot be read.
fn of_lockfile(
    previous: Option<&Validation>,
    (digest, lockfile_path, parsed): (&str, &Path, Option<&Lockfile>),
    workspace_root: &Path,
) -> Option<LocalDependencies> {
    if let Some(recorded) =
        previous.and_then(|previous| previous.local_dependencies.get(digest))
    {
        return Some(recorded.clone());
    }
    if let Some(lockfile) = parsed {
        return Some(resolved_by(lockfile, workspace_root));
    }
    let lockfile = Lockfile::load_from_path(lockfile_path).ok()?;
    Some(lockfile.map_or_else(LocalDependencies::default, |lockfile| {
        resolved_by(&lockfile, workspace_root)
    }))
}

/// The local tarballs and the directories `lockfile` resolves. Workspace
/// projects are among the directories, so that the list depends on the
/// lockfile alone; [`changed`] tells them apart.
fn resolved_by(lockfile: &Lockfile, workspace_root: &Path) -> LocalDependencies {
    let mut tarballs = BTreeSet::new();
    let mut directories = BTreeSet::new();
    for metadata in lockfile.packages.iter().flat_map(|packages| packages.values()) {
        match &metadata.resolution {
            LockfileResolution::Tarball(resolution) => {
                tarballs.extend(super::super::local_file_deps::local_tarball_file(
                    resolution,
                    workspace_root,
                ));
            }
            LockfileResolution::Directory(resolution) => {
                directories.insert(pnpm_fs::lexical_normalize(&workspace_root.join(
                    &resolution.directory,
                )));
            }
            _ => {}
        }
    }
    let paths = |set: BTreeSet<PathBuf>| {
        set.into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    };
    LocalDependencies { tarballs: paths(tarballs), directories: paths(directories) }
}

/// Record `current`, the local dependencies of the lockfile with that
/// digest, in `validation`, with each tarball's signature as `recordable`
/// makes it. Carry from `previous` the entries of the other lockfiles the
/// projects in `validation` still name, with the signatures they were
/// recorded with.
pub(super) fn record(
    validation: &mut Validation,
    current: Option<(&str, LocalDependencies)>,
    (previous, digests): (Option<&Validation>, &FileDigests<'_>),
    recordable: impl Fn(FileSignature) -> FileSignature,
) {
    let referenced: HashSet<&str> = referenced_lockfiles(validation).collect();
    let mut recorded = BTreeMap::new();
    let mut signatures = BTreeMap::new();
    let current_digest = current.as_ref().map(|(digest, _)| *digest);
    if let Some((digest, local)) = current.filter(|(digest, _)| referenced.contains(digest)) {
        for tarball in &local.tarballs {
            if let FileDigest::Present(signature) = digests.of(Path::new(tarball)) {
                signatures.insert(tarball.clone(), recordable(signature));
            }
        }
        recorded.insert(digest.to_string(), local);
    }
    let carried = previous
        .into_iter()
        .flat_map(|previous| &previous.local_dependencies)
        .filter(|(digest, _)| {
            referenced.contains(digest.as_str()) && Some(digest.as_str()) != current_digest
        });
    for (digest, local) in carried {
        signatures.extend(carried_signatures(previous, local));
        recorded.insert(digest.clone(), local.clone());
    }
    validation.files.extend(signatures);
    validation.local_dependencies = recorded;
}

fn carried_signatures<'a>(
    previous: Option<&'a Validation>,
    local: &'a LocalDependencies,
) -> impl Iterator<Item = (String, FileSignature)> + 'a {
    local.tarballs
        .iter()
        .filter_map(move |tarball| {
            let signature = previous?.files.get(tarball)?;
            Some((tarball.clone(), signature.clone()))
        })
}

/// The digests of the lockfiles the projects in `validation` were proven
/// against.
fn referenced_lockfiles(validation: &Validation) -> impl Iterator<Item = &str> {
    validation.lockfile
        .iter()
        .chain(validation.lockfiles.values().flatten())
        .map(String::as_str)
}

/// Whether a local dependency `validation` records may have changed: a
/// directory other than `workspace_dirs`, whose contents no signature
/// covers, or a tarball that is no longer the one recorded. A lockfile the
/// projects were proven against without a record of its local dependencies
/// may resolve any, so it counts as changed.
pub(crate) fn changed(validation: &Validation, workspace_dirs: &HashSet<PathBuf>) -> bool {
    if !referenced_lockfiles(validation)
        .all(|digest| validation.local_dependencies.contains_key(digest))
    {
        return true;
    }
    let digests = FileDigests::new(Some(validation));
    validation.local_dependencies
        .values()
        .any(|local| {
            local.directories
                .iter()
                .any(|dir| !workspace_dirs.contains(Path::new(dir)))
                || local.tarballs
                    .iter()
                    .any(|tarball| {
                        let recorded = validation.files
                            .get(tarball)
                            .map(|signature| signature.digest.as_str());
                        recorded.is_none() || digests.of(Path::new(tarball)).digest() != recorded
                    })
        })
}

#[cfg(test)]
mod tests;

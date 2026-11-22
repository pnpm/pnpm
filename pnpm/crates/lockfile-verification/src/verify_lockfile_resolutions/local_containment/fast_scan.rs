use pnpm_lockfile::{
    ImporterDepVersion,
    Lockfile,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    ProjectSnapshot,
    ResolvedDependencySpec,
    SnapshotDepRef,
    SnapshotEntry,
    VersionPart,
    is_non_local_filesystem_subdep_spec,
    package_root_link_target,
};

use super::super::{
    local_git_shape::{
        has_local_git_resolution,
        is_local_git_key,
    },
    offline_checks::has_local_filesystem_resolution,
};

/// Single linear pre-scan over `lockfile` returning `false` when no importer,
/// package, or snapshot contains any local (`link:`, `file:`, `directory`,
/// `file:` tarball, `git+file:` / local `git`, or local `peerDependencies`)
/// reference, allowing large registry-only workspaces to skip per-importer BFS.
pub(super) fn lockfile_has_any_local_surface(lockfile: &Lockfile) -> bool {
    importers_have_local_surface(lockfile)
        || packages_have_local_surface(lockfile)
        || snapshots_have_local_surface(lockfile)
}

fn importers_have_local_surface(lockfile: &Lockfile) -> bool {
    lockfile.importers.values().any(importer_has_local_surface)
}

fn importer_has_local_surface(imp: &ProjectSnapshot) -> bool {
    imp.dependencies
        .iter()
        .flatten()
        .chain(imp.dev_dependencies.iter().flatten())
        .chain(imp.optional_dependencies.iter().flatten())
        .any(importer_dep_is_local)
}

fn importer_dep_is_local((alias, dep): (&PkgName, &ResolvedDependencySpec)) -> bool {
    match &dep.version {
        ImporterDepVersion::Link(_) => true,
        other => other.resolved_key(alias).is_some_and(|key| key_is_local(&key)),
    }
}

fn packages_have_local_surface(lockfile: &Lockfile) -> bool {
    lockfile.packages
        .iter()
        .flat_map(|pkgs| pkgs.iter())
        .any(|(key, meta)| package_entry_is_local(key, meta))
}

/// Whether the package `key` is local or declares a local peer, with or
/// without a `packages` entry.
pub(super) fn package_has_local_surface(
    key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    match meta {
        Some(meta) => package_entry_is_local(key, meta),
        None => key_is_local(key),
    }
}

fn package_entry_is_local(key: &PkgNameVerPeer, meta: &PackageMetadata) -> bool {
    key_is_local(key)
        || has_local_filesystem_resolution(&meta.resolution)
        || has_local_git_resolution(&meta.resolution)
        || meta.peer_dependencies
            .iter()
            .flat_map(|peers| peers.values())
            .any(|spec| {
                spec.strip_prefix("link:")
                    .and_then(package_root_link_target)
                    .is_none()
                    && is_non_local_filesystem_subdep_spec(spec, |_| false)
            })
}

fn snapshots_have_local_surface(lockfile: &Lockfile) -> bool {
    lockfile.snapshots
        .iter()
        .flat_map(|snaps| snaps.iter())
        .any(|(key, snap)| key_is_local(key) || snapshot_refs_local(snap))
}

fn snapshot_refs_local(snap: &SnapshotEntry) -> bool {
    snap.dependencies
        .iter()
        .flatten()
        .chain(snap.optional_dependencies.iter().flatten())
        .any(|(alias, dep_ref)| match dep_ref {
            SnapshotDepRef::Link(target) => package_root_link_target(target).is_none(),
            other => other.resolve(alias).is_some_and(|child_key| key_is_local(&child_key)),
        })
}

fn key_is_local(key: &PkgNameVerPeer) -> bool {
    matches!(key.suffix.version(), VersionPart::File(_)) || is_local_git_key(key)
}

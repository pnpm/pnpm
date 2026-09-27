//! Taking a global-virtual-store slot for a build.

use super::{
    super::{
        BuildModulesError, NEEDS_BUILD_MARKER, PackageKey, Path, PathBuf, is_started_build_marker,
        mark_global_virtual_store_build_started,
    },
    BuildCandidate, BuildOneSnapshot,
};
use pnpm_lockfile::LockfileResolution;

/// The package directory to build in, with the lock that serializes a
/// build into an isolated global-virtual-store slot with every other
/// install's build of it. The slot is marked mid-build before it is
/// returned. `None` when there is nothing to build: the snapshot has no
/// directory, its global-virtual-store slot carries no marker because an
/// earlier install already built it, another install built the slot while
/// this one waited for its lock, or another install's build of it did not
/// finish. An explicit rebuild still builds a slot without a marker.
pub(super) fn slot_to_build(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Result<Option<(PathBuf, Option<pnpm_fs::DirLock>)>, BuildModulesError> {
    let Some(pkg_dir) = context
        .pkg_roots()
        .canonical(snapshot_key)
        .filter(|dir| dir.exists())
    else {
        return Ok(None);
    };
    if context.directories.pkg_roots_by_key.is_some()
        || !(candidate.patch.is_some() || candidate.should_run_scripts)
    {
        return Ok(Some((pkg_dir, None)));
    }
    let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
    if built_by_an_earlier_install(context, candidate, &marker)? {
        return Ok(None);
    }
    let awaiting_build = marker.is_file();
    let lock = crate::gvs_slot_lock::lock_global_virtual_store_slot(
        context.directories.layout,
        snapshot_key,
    );
    let started = is_started_build_marker(&marker)
        .map_err(|source| BuildModulesError::ReadBuildMarker { path: marker.clone(), source })?;
    if started || (awaiting_build && !marker.is_file()) {
        return Ok(None);
    }
    mark_global_virtual_store_build_started(context.pkg_roots(), snapshot_key);
    Ok(Some((pkg_dir, lock)))
}

/// Whether the global-virtual-store slot `marker` belongs to was built by an
/// earlier install: every import of a slot that needs a build writes the
/// marker, and a finished build removes it. A directory dependency is left
/// out, since its files, and whether they need a build, can change under
/// the same slot.
///
/// The slot is imported in place, so a concurrent install may still be
/// importing it. The import places its completion marker, `package.json`,
/// after every other file, the build marker included, so the slot counts as
/// built only once `package.json` is there and the build marker is not;
/// checked in that order.
fn built_by_an_earlier_install(
    context: &BuildOneSnapshot<'_>,
    candidate: &BuildCandidate<'_>,
    marker: &Path,
) -> Result<bool, BuildModulesError> {
    let from_an_immutable_source = context.graph.packages
        .and_then(|packages| packages.get(&candidate.metadata_key))
        .is_some_and(|metadata| !matches!(metadata.resolution, LockfileResolution::Directory(_)));
    if candidate.force_rebuild
        || !from_an_immutable_source
        || !context.directories.layout.enable_global_virtual_store()
    {
        return Ok(false);
    }
    let exists = |path: &Path| {
        path.try_exists()
            .map_err(|source| BuildModulesError::ReadBuildMarker {
                path: path.to_path_buf(),
                source,
            })
    };
    Ok(exists(&marker.with_file_name("package.json"))? && !exists(marker)?)
}

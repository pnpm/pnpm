//! Taking a global-virtual-store slot for a build.

use super::{
    super::{
        NEEDS_BUILD_MARKER, PackageKey, PathBuf, is_started_build_marker,
        mark_global_virtual_store_build_started,
    },
    BuildCandidate, BuildOneSnapshot,
};

/// The package directory to build in, with the lock that serializes a
/// build into an isolated global-virtual-store slot with every other
/// install's build of it. The slot is marked mid-build before it is
/// returned. `None` when there is nothing to build: the snapshot has no
/// directory, another install built the slot while this one waited for
/// its lock, or another install's build of it did not finish.
pub(super) fn slot_to_build(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Option<(PathBuf, Option<pnpm_fs::DirLock>)> {
    let pkg_dir = context
        .pkg_roots()
        .canonical(snapshot_key)
        .filter(|dir| dir.exists())?;
    if context.directories.pkg_roots_by_key.is_some()
        || !(candidate.patch.is_some() || candidate.should_run_scripts)
    {
        return Some((pkg_dir, None));
    }
    let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
    let awaiting_build = marker.is_file();
    let lock = crate::gvs_slot_lock::lock_global_virtual_store_slot(
        context.directories.layout,
        snapshot_key,
    );
    if is_started_build_marker(&marker) || (awaiting_build && !marker.is_file()) {
        return None;
    }
    mark_global_virtual_store_build_started(context.pkg_roots(), snapshot_key);
    Some((pkg_dir, lock))
}

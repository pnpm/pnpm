//! The package directory a project-local build runs in.

use super::{
    super::{BuildModulesError, PackageKey, PathBuf},
    BuildOneSnapshot,
};

/// The package directory to build in. `None` when the snapshot has no
/// directory: it was skipped, or never linked.
pub(super) fn slot_to_build(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
) -> Result<Option<PathBuf>, BuildModulesError> {
    Ok(context
        .pkg_roots()
        .canonical(snapshot_key)
        .filter(|dir| dir.exists()))
}

//! Which snapshots the build phase may build, patch, or restore a cached
//! build into.

use crate::{UnbuiltBuilds, linking::LinkPhaseInputs};
use pnpm_lockfile::PackageKey;
use std::collections::HashSet;

/// See [`crate::BuildGraphInputs::build_scope`].
pub(crate) fn in_build_scope(
    build_scope: Option<&HashSet<PackageKey>>,
    snapshot_key: &PackageKey,
) -> bool {
    build_scope.is_none_or(|scope| scope.contains(snapshot_key))
}

/// Whether `unbuilt` records the snapshot at `dep_path`, under any of the
/// keys [`UnbuiltBuilds`] holds.
pub(crate) fn records_unbuilt(unbuilt: &UnbuiltBuilds, dep_path: &str) -> bool {
    !unbuilt.is_empty()
        && (unbuilt.contains(dep_path)
            || unbuilt.contains(pnpm_deps_path::get_pkg_id_with_patch_hash(dep_path))
            || unbuilt.contains(pnpm_deps_path::remove_suffix(dep_path)))
}

/// The snapshots an isolated install with project-local slots may build:
/// the ones it materialized, and the ones the previous install recorded as
/// not built. A slot the install left in place keeps the build output and
/// patches the previous install gave it, as a present package does under
/// the hoisted linker.
///
/// `None`, which builds every candidate, when every package must reach the
/// build phase ([`crate::PriorHoistedState::build_present_packages`]), when
/// the caller did not report what it materialized, and for the other
/// linkers: the hoisted linker scopes its build through its package roots,
/// and a global virtual store slot is checked against its build marker.
pub(crate) fn isolated_build_scope(inputs: &LinkPhaseInputs<'_>) -> Option<HashSet<PackageKey>> {
    if inputs.ctx.is_hoisted()
        || inputs.ctx.linker.layout.enable_global_virtual_store()
        || inputs.prior.build_present_packages
    {
        return None;
    }
    let materialized = inputs.graph.materialized_snapshots?;
    let unbuilt = inputs.prior.unbuilt_builds;
    let recorded_unbuilt = inputs.graph.lockfile.snapshots
        .iter()
        .flatten()
        .filter(|_| !unbuilt.is_empty())
        .map(|(snapshot_key, _)| snapshot_key)
        .filter(|snapshot_key| records_unbuilt(unbuilt, &snapshot_key.to_string()));
    Some(
        materialized
            .iter()
            .chain(recorded_unbuilt)
            .cloned()
            .collect(),
    )
}

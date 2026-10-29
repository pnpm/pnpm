//! One directory dependency's own freshness: the manifest the resolver would
//! read for it, whether this scope has checked it already, and the peers its
//! own dependencies shadow.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, InjectedDirectory, LocalDepContext,
    children::check_local_dep_children_freshness,
    peers::PeerShadowing,
    recorded::check_recorded_children_freshness,
    scope::{AliasScope, Visited, snapshot_aliases},
    unhooked_manifest,
};
use pnpm_package_manifest::PackageManifest;

/// The manifest the resolver read for `dep`: the hooked one the check holds,
/// else [`overridden_unhooked_manifest`].
fn read_and_override_manifest(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
) -> Result<PackageManifest, FreshnessCheckError> {
    let hooked = check.workspace.directory_manifests.get(dep.rel_path, dep.declared_as_file);
    if let Some(manifest) = hooked {
        return Ok(manifest.clone());
    }
    let injected = InjectedDirectory {
        dir: dep.dir,
        declared_as_file: dep.declared_as_file,
        workspace_root: dep.dirs.workspace_root,
        manifests_by_dir: check.workspace.manifests_by_dir,
    };
    overridden_unhooked_manifest(&injected, check.parsed_overrides, check.lockfile_dir)
        .ok_or_else(|| dep.outdated())
}

/// [`unhooked_manifest`] with the overrides applied, as the check reads a
/// directory package that no hooked manifest covers.
pub(in crate::install::lockfile_freshness) fn overridden_unhooked_manifest(
    injected: &InjectedDirectory<'_>,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    lockfile_dir: &std::path::Path,
) -> Option<PackageManifest> {
    let mut manifest = unhooked_manifest(injected)?;
    if let Some(parsed) = parsed_overrides {
        crate::VersionsOverrider::new(parsed, lockfile_dir)
            .apply(&mut manifest, Some(injected.dir));
    }
    Some(manifest)
}

pub(super) fn check_single_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    scope: &AliasScope,
    dep: &LocalDepContext<'_>,
    (pkg_key, snapshot, pkg_meta): (
        &pnpm_lockfile::PackageKey,
        Option<&pnpm_lockfile::SnapshotEntry>,
        &pnpm_lockfile::PackageMetadata,
    ),
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let local_manifest = read_and_override_manifest(check, dep)?;
    let Some(snapshot) = snapshot else {
        return Err(dep.outdated());
    };
    // A revisit in the same peer scope is a cycle or a shared dependency
    // already checked.
    if !visited.first_visit(pkg_key, &local_manifest, scope) {
        return Ok(());
    }
    let peers = PeerShadowing::of(
        &local_manifest,
        |name| scope.contains(name),
        check.config.auto_install_peers,
    );
    let children_scope: AliasScope = scope
        .iter()
        .cloned()
        .chain(snapshot_aliases(snapshot))
        .collect();
    match pkg_meta.specifiers.as_ref() {
        Some(recorded) => check_recorded_children_freshness(
            check,
            (dep, &children_scope),
            (&local_manifest, recorded),
            snapshot,
            &peers.shadowed,
            visited,
        )?,
        None => check_local_dep_children_freshness(
            check,
            (dep, &children_scope),
            &local_manifest,
            snapshot,
            &peers.shadowed,
            visited,
        )?,
    }
    peers.check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta)
}

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext,
    children::check_recorded_local_targets,
    peers::PeerShadowing,
    scope::{AliasScope, Visited, snapshot_aliases},
};
use pnpm_lockfile::{PackageMetadata, SnapshotEntry};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    declared_peer_dependencies, declared_specifiers, lockfile_peer_blocks,
};

pub(super) fn check_recorded_directory(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &AliasScope),
    (manifest, snapshot, metadata): (&PackageManifest, &SnapshotEntry, &PackageMetadata),
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    check_inputs(dep, manifest, metadata)?;
    let peers =
        PeerShadowing::of(manifest, |name| scope.contains(name), check.config.auto_install_peers);
    let children_scope = scope
        .iter()
        .cloned()
        .chain(snapshot_aliases(snapshot))
        .collect();
    check_recorded_local_targets(
        check,
        (dep, &children_scope),
        manifest,
        snapshot,
        &peers.shadowed,
        visited,
    )?;
    check_snapshot_directories(check, (dep, &children_scope), snapshot, visited)
}

fn check_inputs(
    dep: &LocalDepContext<'_>,
    manifest: &PackageManifest,
    metadata: &PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let recorded = metadata.specifiers.as_ref().expect("the caller selected recorded inputs");
    let mut declared = declared_specifiers(manifest.value(), dep.name).map_err(|_| dep.outdated())?;
    declared.manifest_source = recorded.manifest_source;
    let (peers, meta) = lockfile_peer_blocks(&declared_peer_dependencies(manifest.value()));
    if declared != *recorded
        || peers != metadata.peer_dependencies
        || meta != metadata.peer_dependencies_meta
    {
        return Err(dep.outdated());
    }
    Ok(())
}

fn check_snapshot_directories(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &AliasScope),
    snapshot: &SnapshotEntry,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let optional =
        check.config.optional.then_some(snapshot.optional_dependencies.as_ref()).flatten();
    for (alias, reference) in [snapshot.dependencies.as_ref(), optional]
        .into_iter()
        .flatten()
        .flatten()
    {
        let Some(key) = reference.resolve(alias) else { continue };
        let Some(metadata) = check.lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&key.without_peer()))
            .filter(|metadata| metadata.specifiers.is_some())
        else {
            continue;
        };
        let pnpm_lockfile::LockfileResolution::Directory(resolution) = &metadata.resolution else {
            continue;
        };
        let directory = pnpm_resolving_deps_resolver::directory_package_dir(
            check.lockfile_dir,
            &resolution.directory,
        );
        let alias = alias.to_string();
        let child = dep.child(&alias, "workspace:*", &resolution.directory, &directory);
        let child_snapshot = check.lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&key));
        super::package::check_single_directory_dep_freshness(
            check,
            scope,
            &child,
            (&key, child_snapshot, metadata),
            visited,
        )?;
    }
    Ok(())
}

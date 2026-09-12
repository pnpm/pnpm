use super::{
    DependencyGroup, DeployError, HashMap, HashSet, Lockfile, PackageKey, PkgName, PkgNameVerPeer,
    ProjectInfo, ProjectSnapshot, SnapshotDepRef, SnapshotEntry, Value, VecDeque,
};

/// A linked workspace package has no package snapshot in the shared lockfile,
/// so the importer its deployed snapshot is synthesized from carries no peer
/// bindings. Bind each still-unresolved peer to the deployed graph's own
/// resolution while that resolution is unambiguous, and refuse when it is not:
/// picking between candidates is precisely the decision that injecting the
/// package would have made, and it cannot be recovered afterwards.
pub(super) fn bind_singleton_peers(
    lockfile: &mut Lockfile,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, ProjectInfo>,
) -> miette::Result<()> {
    if linked_workspace_projects.is_empty() {
        return Ok(());
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return Ok(()) };

    let candidates = resolution_candidates(lockfile, snapshots);
    let bindings = collect_peer_bindings(snapshots, &candidates, linked_workspace_projects)?;

    let Some(snapshots) = lockfile.snapshots.as_mut() else { return Ok(()) };
    for (package_key, peer, reference) in bindings {
        if let Some(snapshot) = snapshots.get_mut(&package_key) {
            snapshot.dependencies.get_or_insert_default().insert(peer, reference);
        }
    }
    Ok(())
}

/// The `(package, peer, reference)` triples the deployed graph can bind
/// unambiguously.
fn collect_peer_bindings(
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    candidates: &HashMap<PkgName, HashSet<PkgNameVerPeer>>,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, ProjectInfo>,
) -> miette::Result<Vec<(PkgNameVerPeer, PkgName, SnapshotDepRef)>> {
    let mut bindings = Vec::new();
    for (package_key, project) in linked_workspace_projects {
        if !snapshots.contains_key(package_key) {
            continue;
        }
        for peer in &project.peer_dependencies {
            if let Some(binding) =
                singleton_peer_binding(snapshots, candidates, package_key, project, peer)?
            {
                bindings.push((package_key.clone(), peer.clone(), binding));
            }
        }
    }
    Ok(bindings)
}

/// Every snapshot key the deployed graph resolves, keyed by package name
/// rather than by the reference that spelled it, so an npm-aliased edge
/// and a plain one that name the same package count once.
fn resolution_candidates(
    lockfile: &Lockfile,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
) -> HashMap<PkgName, HashSet<PkgNameVerPeer>> {
    let importer_keys =
        lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY).into_iter().flat_map(|importer| {
            [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|(alias, dependency)| dependency.version.resolved_key(alias))
        });
    let snapshot_keys = snapshots.values().flat_map(|snapshot| {
        [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|(alias, dependency)| dependency.resolve(alias))
    });
    let mut candidates: HashMap<PkgName, HashSet<PkgNameVerPeer>> = HashMap::new();
    for key in importer_keys.chain(snapshot_keys) {
        candidates.entry(key.name.clone()).or_default().insert(key);
    }
    candidates
}

/// The reference one still-unresolved peer binds to, if the deployed
/// graph resolves it unambiguously.
fn singleton_peer_binding(
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    candidates: &HashMap<PkgName, HashSet<PkgNameVerPeer>>,
    package_key: &PkgNameVerPeer,
    project: &ProjectInfo,
    peer: &PkgName,
) -> miette::Result<Option<SnapshotDepRef>> {
    // Either map already binding the peer counts: re-binding one the
    // package declares as an optional dependency would copy it into the
    // required map and quietly promote it.
    // The graph prune clears the optional map before this runs, so a peer
    // the package depends on optionally is invisible in the snapshot under
    // `--no-optional`. Binding it there would resurrect a dependency the
    // flag excluded.
    if project.declared_dependencies.contains(peer) {
        return Ok(None);
    }
    let bound = snapshots.get(package_key).is_some_and(|snapshot| {
        [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
            .into_iter()
            .flatten()
            .any(|dependencies| dependencies.contains_key(peer))
    });
    if bound {
        return Ok(None);
    }
    // A peer the deployed graph does not provide at all stays unresolved,
    // exactly as it is in the workspace this deploy was taken from.
    let Some(resolutions) = candidates.get(peer) else { return Ok(None) };
    if resolutions.len() > 1 {
        let mut versions = resolutions.iter().map(|key| key.suffix.to_string()).collect::<Vec<_>>();
        versions.sort();
        return Err(DeployError::AmbiguousPeer {
            package: project.name.clone().unwrap_or_else(|| package_key.to_string()),
            peer: peer.to_string(),
            versions: versions.join(", "),
        }
        .into());
    }
    Ok(resolutions.iter().next().map(|resolution| SnapshotDepRef::Plain(resolution.suffix.clone())))
}

/// Keep only the dependency graph that the deploy install will materialize.
///
/// The deploy importer already carries just the included dependency groups, so
/// this walks it in full: `deploy --prod` excludes dev-only and unrelated
/// workspace snapshots from both the lockfile and the localized virtual store.
pub(super) fn prune_deploy_lockfile_graph(
    lockfile: &mut Lockfile,
    dependency_groups: &[DependencyGroup],
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return };
    let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) else { return };

    let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
    let reachable = reachable_deploy_snapshots(importer, snapshots, include_optional);

    let reachable_metadata = reachable.iter().map(PackageKey::without_peer).collect::<HashSet<_>>();
    retain_reachable_snapshots(lockfile, &reachable, include_optional);
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| reachable_metadata.contains(key));
        if packages.is_empty() {
            lockfile.packages = None;
        }
    }
}

fn retain_reachable_snapshots(
    lockfile: &mut Lockfile,
    reachable: &HashSet<PkgNameVerPeer>,
    include_optional: bool,
) {
    let Some(snapshots) = lockfile.snapshots.as_mut() else { return };
    snapshots.retain(|key, _| reachable.contains(key));
    if !include_optional {
        // A retained snapshot's optional edges point at packages this
        // prune just dropped.
        for snapshot in snapshots.values_mut() {
            snapshot.optional_dependencies = None;
        }
    }
    if snapshots.is_empty() {
        lockfile.snapshots = None;
    }
}

/// Every snapshot the deployed root importer can reach.
fn reachable_deploy_snapshots(
    importer: &ProjectSnapshot,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    include_optional: bool,
) -> HashSet<PkgNameVerPeer> {
    let mut queue: VecDeque<PkgNameVerPeer> = [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .filter_map(|(alias, dependency)| dependency.version.resolved_key(alias))
    .filter(|key| snapshots.contains_key(key))
    .collect();

    let mut reachable = HashSet::new();
    while let Some(key) = queue.pop_front() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        queue.extend(
            snapshot
                .dependencies
                .as_ref()
                .into_iter()
                .chain(
                    include_optional.then_some(snapshot.optional_dependencies.as_ref()).flatten(),
                )
                .flatten()
                .filter_map(|(alias, dependency)| dependency.resolve(alias))
                .filter(|child| snapshots.contains_key(child)),
        );
    }
    reachable
}

pub(super) fn omit_peers_of_excluded_dependencies(
    manifest: &mut Value,
    declared_dependencies: &HashSet<String>,
    target_snapshot: &ProjectSnapshot,
) {
    let included_dependencies = dependency_names(target_snapshot);
    let excluded_dependencies =
        declared_dependencies.difference(&included_dependencies).cloned().collect::<HashSet<_>>();
    let Some(manifest) = manifest.as_object_mut() else { return };
    for field in ["peerDependencies", "peerDependenciesMeta"] {
        if let Some(Value::Object(dependencies)) = manifest.get_mut(field) {
            dependencies.retain(|name, _| !excluded_dependencies.contains(name));
        }
    }
}

fn dependency_names(snapshot: &ProjectSnapshot) -> HashSet<String> {
    snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(snapshot.dev_dependencies.iter().flatten())
        .chain(snapshot.optional_dependencies.iter().flatten())
        .map(|(name, _)| name.to_string())
        .collect()
}

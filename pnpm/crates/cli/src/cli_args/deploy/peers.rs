use super::{
    Config, ConvertCtx, DependencyGroup, DeployError, HashMap, HashSet, ImporterDepVersion,
    Lockfile, PackageKey, PeerSatisfactionEdges, PkgName, PkgNameVerPeer, ProjectInfo,
    ProjectSnapshot, ResolveBases, SnapshotDepRef, SnapshotEntry, Value, VecDeque,
    convert_importer_version_to_snapshot_ref, convert_package_key,
};

mod deployed_graph;

use deployed_graph::{
    DeployedGraph, ancestor_peer_providers, dependents_by_snapshot, is_package_root_link,
    resolution_candidates, snapshot_dependencies,
};

/// A workspace package the deployed graph links rather than injects.
pub(super) struct LinkedWorkspaceProject {
    project: ProjectInfo,
    /// The project's dev dependencies that are also its peers, recorded only
    /// in an injected workspace. There a workspace package is linked rather
    /// than injected only when its injected resolution matched its own
    /// importer, dev dependencies included, so a peer it also lists as a dev
    /// dependency was bound to exactly that.
    deduped_peer_resolutions: Option<HashMap<PkgName, SnapshotDepRef>>,
}

impl LinkedWorkspaceProject {
    pub(super) fn new(
        project: ProjectInfo,
        lockfile: &Lockfile,
        importer: &ProjectSnapshot,
        ctx: &ConvertCtx<'_>,
        bases: &ResolveBases,
    ) -> Self {
        let injected_workspace =
            lockfile.settings.as_ref().is_some_and(|settings| settings.inject_workspace_packages);
        if !injected_workspace {
            return LinkedWorkspaceProject { project, deduped_peer_resolutions: None };
        }
        // A reference the conversion rejects, such as a link outside the
        // workspace, cannot name a deployed snapshot, since every snapshot key
        // passed the same conversion. It is skipped rather than failing a
        // deploy that may not even include this package.
        let deduped_peer_resolutions = importer.dev_dependencies
            .iter()
            .flatten()
            .filter(|(name, _)| project.peer_dependencies.contains(name))
            .filter_map(|(name, spec)| {
                convert_importer_version_to_snapshot_ref(name, &spec.version, ctx, bases)
                    .ok()
                    .map(|reference| (name.clone(), reference))
            })
            .collect::<HashMap<_, _>>();
        LinkedWorkspaceProject { project, deduped_peer_resolutions: Some(deduped_peer_resolutions) }
    }

    /// The reference `peer` binds to through the deduped resolutions, if the
    /// deployed graph has it.
    fn deduped_peer_binding(
        &self,
        snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
        peer: &PkgName,
    ) -> Option<SnapshotDepRef> {
        let reference = self.deduped_peer_resolutions.as_ref()?.get(peer)?;
        reference
            .resolve(peer)
            .is_some_and(|key| snapshots.contains_key(&key))
            .then(|| reference.clone())
    }
}

/// A linked workspace package has no package snapshot in the shared lockfile,
/// so the importer its deployed snapshot is synthesized from carries no peer
/// bindings. Bind each still-unresolved peer the way injecting the package
/// would have: to what its parents provide, see [`ancestor_peer_providers`].
/// A peer no ancestor provides binds to the deployed graph's own resolution
/// while that is unambiguous. Deploy refuses when either choice is
/// ambiguous: picking between candidates is precisely the decision that
/// injecting the package would have made, and it cannot be recovered
/// afterwards.
pub(super) fn bind_singleton_peers(
    lockfile: &mut Lockfile,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, LinkedWorkspaceProject>,
) -> miette::Result<()> {
    if linked_workspace_projects.is_empty() {
        return Ok(());
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return Ok(()) };
    let importer = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY);

    let graph = DeployedGraph {
        importer,
        snapshots,
        candidates: resolution_candidates(importer, snapshots),
        dependents: dependents_by_snapshot(importer, snapshots, |parent, alias| {
            declares_peer(lockfile, linked_workspace_projects, parent, alias)
        }),
    };
    let bindings = collect_peer_bindings(&graph, linked_workspace_projects)?;

    let Some(snapshots) = lockfile.snapshots.as_mut() else { return Ok(()) };
    for (package_key, peer, reference) in bindings {
        if let Some(snapshot) = snapshots.get_mut(&package_key) {
            snapshot.dependencies.get_or_insert_default().insert(peer, reference);
        }
    }
    Ok(())
}

/// Whether the package at `key` declares `alias` as a peer dependency, in its
/// lockfile metadata or, for a linked workspace package, in its manifest. A
/// linked package that also depends on `alias` resolves it as that
/// dependency.
fn declares_peer(
    lockfile: &Lockfile,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, LinkedWorkspaceProject>,
    key: &PkgNameVerPeer,
    alias: &PkgName,
) -> bool {
    if let Some(linked) = linked_workspace_projects.get(key) {
        return linked.project.peer_dependencies.contains(alias)
            && !linked.project.declared_dependencies.contains(alias);
    }
    lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .is_some_and(|peers| peers.contains_key(&alias.to_string()))
}

/// The `(package, peer, reference)` triples the deployed graph can bind
/// unambiguously.
fn collect_peer_bindings(
    graph: &DeployedGraph<'_>,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, LinkedWorkspaceProject>,
) -> miette::Result<Vec<(PkgNameVerPeer, PkgName, SnapshotDepRef)>> {
    let mut bindings = Vec::new();
    for (package_key, linked) in linked_workspace_projects {
        if !graph.snapshots.contains_key(package_key) {
            continue;
        }
        for peer in &linked.project.peer_dependencies {
            let site = LinkedPeer { package_key, linked, peer };
            if let Some(binding) = singleton_peer_binding(graph, &site)? {
                bindings.push((package_key.clone(), peer.clone(), binding));
            }
        }
    }
    Ok(bindings)
}

/// One peer of one linked workspace package.
struct LinkedPeer<'a> {
    package_key: &'a PkgNameVerPeer,
    linked: &'a LinkedWorkspaceProject,
    peer: &'a PkgName,
}

impl LinkedPeer<'_> {
    /// Whether the package already binds the peer, or declares it as a
    /// dependency.
    ///
    /// Either map already binding the peer counts: re-binding one the package
    /// declares as an optional dependency would copy it into the required map
    /// and quietly promote it. The graph prune clears the optional map before
    /// this runs, so a peer the package depends on optionally is invisible in
    /// the snapshot under `--no-optional`. Binding it there would resurrect a
    /// dependency the flag excluded.
    fn is_bound(&self, graph: &DeployedGraph<'_>) -> bool {
        self.linked.project.declared_dependencies.contains(self.peer)
            || graph.snapshots
                .get(self.package_key)
                .is_some_and(|snapshot| {
                    snapshot_dependencies(snapshot).any(|(alias, _)| alias == self.peer)
                })
    }

    fn ambiguous(&self, versions: impl Iterator<Item = String>) -> miette::Report {
        let mut versions = versions.collect::<Vec<_>>();
        versions.sort();
        DeployError::AmbiguousPeer {
            package: self.linked.project.name
                .clone()
                .unwrap_or_else(|| self.package_key.to_string()),
            peer: self.peer.to_string(),
            versions: versions.join(", "),
        }
        .into()
    }
}

/// The reference one still-unresolved peer binds to, if the deployed
/// graph resolves it unambiguously.
fn singleton_peer_binding(
    graph: &DeployedGraph<'_>,
    site: &LinkedPeer<'_>,
) -> miette::Result<Option<SnapshotDepRef>> {
    if site.is_bound(graph) {
        return Ok(None);
    }
    if let Some(reference) = site.linked.deduped_peer_binding(graph.snapshots, site.peer) {
        return Ok(Some(reference));
    }
    if let Some(reference) = ancestor_peer_binding(graph, site)? {
        return Ok(Some(reference));
    }
    graph_peer_binding(graph, site)
}

/// What the ancestors of the package provide for the peer, see
/// [`ancestor_peer_providers`].
fn ancestor_peer_binding(
    graph: &DeployedGraph<'_>,
    site: &LinkedPeer<'_>,
) -> miette::Result<Option<SnapshotDepRef>> {
    match ancestor_peer_providers(graph, site.package_key, site.peer).as_slice() {
        [] => Ok(None),
        [reference] if is_package_root_link(reference) => Ok(None),
        [reference] => Ok(Some(reference.clone())),
        providers => Err(site.ambiguous(
            providers
                .iter()
                .map(|reference| match reference.resolve(site.peer) {
                    Some(key) if &key.name == site.peer => key.suffix.to_string(),
                    Some(key) => key.to_string(),
                    None => reference.to_string(),
                }),
        )),
    }
}

/// The deployed graph's only resolution of the peer.
fn graph_peer_binding(
    graph: &DeployedGraph<'_>,
    site: &LinkedPeer<'_>,
) -> miette::Result<Option<SnapshotDepRef>> {
    // A peer the deployed graph does not provide at all stays unresolved,
    // exactly as it is in the workspace this deploy was taken from.
    let Some(resolutions) = graph.candidates.get(site.peer) else { return Ok(None) };
    if resolutions.len() > 1 {
        return Err(site.ambiguous(resolutions.iter().map(|key| key.suffix.to_string())));
    }
    Ok(resolutions
        .iter()
        .next()
        .map(|resolution| SnapshotDepRef::Plain(resolution.suffix.clone())))
}

/// The source lockfile's peer-satisfaction edges, under the keys the deployed
/// lockfile gives their snapshots. They are classified over the workspace's
/// own importers, since the deployed lockfile keeps only the deployed project.
/// Empty when the deploy installs every dependency group.
pub(super) fn deploy_peer_edges(
    lockfile: &Lockfile,
    config: &Config,
    dependency_groups: &[DependencyGroup],
    ctx: &ConvertCtx<'_>,
) -> miette::Result<PeerSatisfactionEdges> {
    let every_group = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .iter()
        .all(|group| dependency_groups.contains(group));
    if every_group {
        return Ok(PeerSatisfactionEdges::default());
    }
    PeerSatisfactionEdges::of_lockfile(lockfile, config.peer_edge_options())
        .iter()
        .map(|(key, aliases)| Ok((convert_package_key(key, ctx)?, aliases.clone())))
        .collect()
}

/// Keep only the dependency graph that the deploy install will materialize.
///
/// The deploy importer already carries just the included dependency groups, so
/// this walks it in full: `deploy --prod` excludes dev-only and unrelated
/// workspace snapshots from both the lockfile and the localized virtual store.
/// The walk leaves out `peer_edges`, and a retained snapshot loses those of
/// them whose target the prune drops.
pub(super) fn prune_deploy_lockfile_graph(
    lockfile: &mut Lockfile,
    dependency_groups: &[DependencyGroup],
    peer_edges: &PeerSatisfactionEdges,
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return };
    let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) else {
        return;
    };

    let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
    let reachable = reachable_deploy_snapshots(importer, snapshots, include_optional, peer_edges);

    let reachable_metadata = reachable
        .iter()
        .map(PackageKey::without_peer)
        .collect::<HashSet<_>>();
    retain_reachable_snapshots(lockfile, &reachable, include_optional, peer_edges);
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
    peer_edges: &PeerSatisfactionEdges,
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
    peer_edges.prune_dangling(snapshots);
    if snapshots.is_empty() {
        lockfile.snapshots = None;
    }
}

/// Every snapshot the deployed root importer can reach.
fn reachable_deploy_snapshots(
    importer: &ProjectSnapshot,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    include_optional: bool,
    peer_edges: &PeerSatisfactionEdges,
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
            peer_edges
                .followed_entries(&key, snapshot, include_optional)
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
    let excluded_dependencies = declared_dependencies
        .difference(&included_dependencies)
        .cloned()
        .collect::<HashSet<_>>();
    let Some(manifest) = manifest.as_object_mut() else { return };
    for field in ["peerDependencies", "peerDependenciesMeta"] {
        if let Some(Value::Object(dependencies)) = manifest.get_mut(field) {
            dependencies.retain(|name, _| !excluded_dependencies.contains(name));
        }
    }
}

fn dependency_names(snapshot: &ProjectSnapshot) -> HashSet<String> {
    snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.dev_dependencies.iter().flatten())
        .chain(snapshot.optional_dependencies.iter().flatten())
        .map(|(name, _)| name.to_string())
        .collect()
}

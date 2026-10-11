use super::{
    HashMap, HashSet, ImporterDepVersion, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotDepRef,
    SnapshotEntry, VecDeque,
};

/// The pruned deployed lockfile, indexed for binding the peers of linked
/// workspace packages.
pub(super) struct DeployedGraph<'a> {
    pub(super) importer: Option<&'a ProjectSnapshot>,
    pub(super) snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
    pub(super) candidates: HashMap<PkgName, HashSet<PkgNameVerPeer>>,
    pub(super) dependents: HashMap<PkgNameVerPeer, Vec<Dependent<'a>>>,
}

/// A node of the deployed graph that depends on a snapshot.
#[derive(Clone, Copy)]
pub(super) enum Dependent<'a> {
    /// The deployed project.
    Importer,
    Snapshot(&'a PkgNameVerPeer),
}

/// Every snapshot key the deployed graph resolves, keyed by package name
/// rather than by the reference that spelled it, so an npm-aliased edge
/// and a plain one that name the same package count once.
pub(super) fn resolution_candidates(
    importer: Option<&ProjectSnapshot>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
) -> HashMap<PkgName, HashSet<PkgNameVerPeer>> {
    let importer_keys =
        importer_dependencies(importer).filter_map(|(alias, version)| version.resolved_key(alias));
    let snapshot_keys = snapshots
        .values()
        .flat_map(snapshot_dependencies)
        .filter_map(|(alias, dependency)| dependency.resolve(alias));
    let mut candidates: HashMap<PkgName, HashSet<PkgNameVerPeer>> = HashMap::new();
    for key in importer_keys.chain(snapshot_keys) {
        candidates
            .entry(key.name.clone())
            .or_default()
            .insert(key);
    }
    candidates
}

/// The nodes that depend on each snapshot of the deployed graph, leaving out
/// an edge from `parent` under an alias that `is_peer_of(parent, alias)`.
pub(super) fn dependents_by_snapshot<'a>(
    importer: Option<&'a ProjectSnapshot>,
    snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
    is_peer_of: impl Fn(&PkgNameVerPeer, &PkgName) -> bool,
) -> HashMap<PkgNameVerPeer, Vec<Dependent<'a>>> {
    let importer_edges = importer_dependencies(importer)
        .filter_map(|(alias, version)| version.resolved_key(alias))
        .map(|child| (child, Dependent::Importer));
    // A snapshot records the package it picked for one of its peers as an
    // ordinary dependency entry, but the snapshot is not that package's
    // parent.
    let snapshot_edges = snapshots
        .iter()
        .flat_map(|(parent, snapshot)| {
            snapshot_dependencies(snapshot)
                .filter(|(alias, _)| !is_peer_of(parent, alias))
                .filter_map(|(alias, dependency)| dependency.resolve(alias))
                .map(move |child| (child, Dependent::Snapshot(parent)))
        })
        .collect::<Vec<_>>();
    let mut dependents: HashMap<PkgNameVerPeer, Vec<Dependent<'a>>> = HashMap::new();
    for (child, dependent) in importer_edges.chain(snapshot_edges) {
        dependents
            .entry(child)
            .or_default()
            .push(dependent);
    }
    dependents
}

/// The deployed project's dependencies in every group it installs.
fn importer_dependencies(
    importer: Option<&ProjectSnapshot>,
) -> impl Iterator<Item = (&PkgName, &ImporterDepVersion)> {
    importer
        .into_iter()
        .flat_map(|importer| {
            [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
        })
        .flatten()
        .flatten()
        .map(|(alias, dependency)| (alias, &dependency.version))
}

pub(super) fn snapshot_dependencies(
    snapshot: &SnapshotEntry,
) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flatten()
}

/// The distinct references the nearest ancestors of `package_key` that depend
/// on `peer` resolve it to, one per path up the graph, the same way
/// injecting the package would resolve its peer from its parents. An
/// ancestor that does not depend on `peer` passes the search on to its own
/// dependents.
///
/// A link into the providing package, see [`is_package_root_link`], names a
/// different package for each provider, so it counts once per provider.
pub(super) fn ancestor_peer_providers(
    graph: &DeployedGraph<'_>,
    package_key: &PkgNameVerPeer,
    peer: &PkgName,
) -> Vec<SnapshotDepRef> {
    let mut search = ProviderSearch {
        providers: Vec::new(),
        visited: HashSet::new(),
        queue: VecDeque::from([package_key]),
        package_root_link_providers: HashSet::new(),
    };
    while let Some(key) = search.queue.pop_front() {
        for dependent in graph.dependents
            .get(key)
            .into_iter()
            .flatten()
        {
            search.visit(graph, *dependent, peer);
        }
    }
    search.providers
}

/// The state of [`ancestor_peer_providers`]'s breadth-first walk.
struct ProviderSearch<'a> {
    providers: Vec<SnapshotDepRef>,
    visited: HashSet<&'a PkgNameVerPeer>,
    queue: VecDeque<&'a PkgNameVerPeer>,
    /// `None` stands for the deployed project.
    package_root_link_providers: HashSet<Option<&'a PkgNameVerPeer>>,
}

impl<'a> ProviderSearch<'a> {
    /// Record what `dependent` provides, or queue it to search its own
    /// dependents.
    fn visit(&mut self, graph: &DeployedGraph<'a>, dependent: Dependent<'a>, peer: &PkgName) {
        match (provided_peer(graph, dependent, peer), dependent) {
            (Some(reference), _) => self.record(reference, dependent, peer),
            (None, Dependent::Snapshot(parent)) if self.visited.insert(parent) => {
                self.queue.push_back(parent);
            }
            (None, _) => {}
        }
    }

    /// Add `reference` unless a provider already names the same package. A
    /// link into the providing package names a different package for each
    /// provider.
    fn record(&mut self, reference: SnapshotDepRef, dependent: Dependent<'a>, peer: &PkgName) {
        let is_new = if is_package_root_link(&reference) {
            self.package_root_link_providers.insert(match dependent {
                Dependent::Importer => None,
                Dependent::Snapshot(provider) => Some(provider),
            })
        } else {
            !self.providers.iter().any(|known| same_target(known, &reference, peer))
        };
        if is_new {
            self.providers.push(reference);
        }
    }
}

/// The reference `dependent` resolves `peer` to, if it depends on it.
fn provided_peer(
    graph: &DeployedGraph<'_>,
    dependent: Dependent<'_>,
    peer: &PkgName,
) -> Option<SnapshotDepRef> {
    match dependent {
        Dependent::Importer => importer_dependencies(graph.importer)
            .find(|(alias, _)| *alias == peer)
            .and_then(|(alias, version)| importer_version_to_snapshot_ref(alias, version)),
        Dependent::Snapshot(key) => graph.snapshots
            .get(key)
            .into_iter()
            .flat_map(snapshot_dependencies)
            .find(|(alias, _)| *alias == peer)
            .map(|(_, reference)| reference.clone()),
    }
}

/// The snapshot reference that names what an importer entry for `alias`
/// resolves to.
fn importer_version_to_snapshot_ref(
    alias: &PkgName,
    version: &ImporterDepVersion,
) -> Option<SnapshotDepRef> {
    if let ImporterDepVersion::Link(target) = version {
        return Some(SnapshotDepRef::Link(target.clone()));
    }
    let key = version.resolved_key(alias)?;
    Some(if &key.name == alias {
        SnapshotDepRef::Plain(key.suffix)
    } else {
        SnapshotDepRef::Alias(key)
    })
}

/// Whether `reference` links into the package that declares it, which reads
/// differently once copied into another package's snapshot.
pub(super) fn is_package_root_link(reference: &SnapshotDepRef) -> bool {
    matches!(reference, SnapshotDepRef::Link(target) if pnpm_lockfile::package_root_link_target(target).is_some())
}

/// Whether two references to `peer` name the same package, however each is
/// spelled.
fn same_target(left: &SnapshotDepRef, right: &SnapshotDepRef, peer: &PkgName) -> bool {
    match (left.resolve(peer), right.resolve(peer)) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests;

use super::{Arc, DependencyGroup, HashMap, HashSet, TreeCtx};
use crate::resolve_dependency_tree::{DependencySpec, direct_dep_specifier_unchanged};
use hashed_suffix::hashed_peer_keys;
use pnpm_lockfile::{PeerBindings, PkgName};

mod hashed_suffix;

/// The peer versions the prior lockfile locked for the importer.
pub(super) struct LockedPeers {
    pub(super) versions: Arc<HashMap<String, HashSet<String>>>,
}

impl LockedPeers {
    pub(super) fn of(ctx: &TreeCtx, importer_id: &str, wanted: &[DependencySpec]) -> Self {
        let workspace = ctx.workspace();
        let lockfile = workspace.wanted_lockfile().map(AsRef::as_ref);
        let versions = match lockfile {
            Some(lockfile) if !lockfile.importers.contains_key(importer_id) => {
                workspace.unlisted_importer_locked_peers(|| all_locked_peer_versions(lockfile))
            }
            Some(lockfile) => {
                let retained = retained_dependency_names(lockfile, importer_id, wanted);
                Arc::new(importer_locked_peer_versions(Some(lockfile), importer_id, |name| {
                    retained.contains(name)
                }))
            }
            None => Arc::default(),
        };
        Self { versions }
    }
}

fn retained_dependency_names(
    lockfile: &pnpm_lockfile::Lockfile,
    importer_id: &str,
    wanted: &[DependencySpec],
) -> HashSet<PkgName> {
    let importer = lockfile.importers.get(importer_id);
    wanted
        .iter()
        .filter(|dep| {
            direct_dep_specifier_unchanged(lockfile, importer, &dep.alias, &dep.bare_specifier)
        })
        .filter_map(|dep| dep.alias.parse().ok())
        .collect()
}

pub(super) fn importer_locked_peer_versions(
    wanted_lockfile: Option<&pnpm_lockfile::Lockfile>,
    importer_id: &str,
    is_retained: impl Fn(&PkgName) -> bool,
) -> HashMap<String, HashSet<String>> {
    let Some(lockfile) = wanted_lockfile else {
        return HashMap::default();
    };
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return all_locked_peer_versions(lockfile);
    };
    let mut versions = HashMap::<String, HashSet<String>>::default();
    let mut bound_cache = hashed_suffix::BoundCache::default();
    for (alias, dependency) in importer.dependencies_by_groups([
        DependencyGroup::Prod,
        DependencyGroup::Optional,
        DependencyGroup::Dev,
    ]) {
        if !is_retained(alias) {
            continue;
        }
        let Some(key) = dependency.version.resolved_key(alias) else {
            continue;
        };
        let snapshot = lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&key));
        for (name, version) in
            locked_peer_versions_for_key_cached(lockfile, &key, snapshot, &mut bound_cache)
        {
            versions
                .entry(name)
                .or_default()
                .insert(version);
        }
    }
    versions
}

/// Every peer version the lockfile pins anywhere, for an importer it does not
/// list.
pub(super) fn all_locked_peer_versions(
    lockfile: &pnpm_lockfile::Lockfile,
) -> HashMap<String, HashSet<String>> {
    let mut versions = HashMap::<String, HashSet<String>>::default();
    let mut bound_cache = hashed_suffix::BoundCache::default();
    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        for (name, version) in
            locked_peer_versions_for_key_cached(lockfile, key, Some(snapshot), &mut bound_cache)
        {
            versions
                .entry(name)
                .or_default()
                .insert(version);
        }
    }
    versions
}

/// Read the suffixes first, then recover missing peer names from snapshot edges.
/// Recovery cannot replace a pin already encoded by an independent provider path.
fn locked_peer_versions_for_key_cached<'lockfile>(
    lockfile: &'lockfile pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&'lockfile pnpm_lockfile::SnapshotEntry>,
    bound_cache: &mut hashed_suffix::BoundCache,
) -> Vec<(String, String)> {
    let mut traversal = LockedPeerTraversal {
        lockfile,
        bound_cache,
        visited: HashSet::default(),
        versions: Vec::new(),
        recovered: Vec::new(),
    };
    traversal.collect_pins(vec![(key.clone(), snapshot)], &HashSet::default());
    let encoded_names = traversal.versions
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    while !traversal.recovered.is_empty() {
        let pending = std::mem::take(&mut traversal.recovered);
        traversal.collect_pins(pending, &encoded_names);
    }
    traversal.versions
}

struct LockedPeerTraversal<'lockfile, 'cache> {
    lockfile: &'lockfile pnpm_lockfile::Lockfile,
    bound_cache: &'cache mut hashed_suffix::BoundCache,
    visited: HashSet<pnpm_lockfile::PkgNameVerPeer>,
    versions: Vec<(String, String)>,
    recovered:
        Vec<(pnpm_lockfile::PkgNameVerPeer, Option<&'lockfile pnpm_lockfile::SnapshotEntry>)>,
}

impl<'lockfile> LockedPeerTraversal<'lockfile, '_> {
    fn collect_pins(
        &mut self,
        mut pending: Vec<(
            pnpm_lockfile::PkgNameVerPeer,
            Option<&'lockfile pnpm_lockfile::SnapshotEntry>,
        )>,
        encoded_names: &HashSet<String>,
    ) {
        while let Some((key, snapshot)) = pending.pop() {
            if !self.visited.insert(key.clone()) {
                continue;
            }
            pending.extend(self.collect_key(&key, snapshot, encoded_names));
        }
    }

    fn collect_key(
        &mut self,
        key: &pnpm_lockfile::PkgNameVerPeer,
        snapshot: Option<&'lockfile pnpm_lockfile::SnapshotEntry>,
        encoded_names: &HashSet<String>,
    ) -> Vec<(pnpm_lockfile::PkgNameVerPeer, Option<&'lockfile pnpm_lockfile::SnapshotEntry>)> {
        let mut pending = Vec::new();
        let peers = locked_peer_keys(self.lockfile, key, snapshot, self.bound_cache);
        let verified_context =
            !self.bound_cache.get(key).is_some_and(pnpm_lockfile::BoundProviders::is_hashed);
        let retained_hash = matches!(PeerBindings::from_key(key), Some(PeerBindings::Hashed))
            && snapshot.is_some_and(|snapshot| snapshot.recorded_peer_bindings.is_some());
        let recover_context = verified_context && !retained_hash;
        for (name, peer) in peers {
            let version = peer.suffix.without_peer().to_string();
            if recover_context && !encoded_names.contains(&name) {
                let recovered = self.recover_providers(snapshot, &name, &peer);
                self.recovered.extend(recovered);
            }
            if verified_context {
                pending.extend(provider_with_peers(self.lockfile, peer));
            }
            if !encoded_names.contains(&name) {
                self.versions.push((name, version));
            }
        }
        pending
    }

    fn recover_providers(
        &mut self,
        snapshot: Option<&'lockfile pnpm_lockfile::SnapshotEntry>,
        name: &str,
        peer: &pnpm_lockfile::PkgNameVerPeer,
    ) -> Vec<(pnpm_lockfile::PkgNameVerPeer, Option<&'lockfile pnpm_lockfile::SnapshotEntry>)> {
        let Some(snapshot) = snapshot else { return Vec::new() };
        let providers = match snapshot_peer_provider(snapshot, name, peer) {
            Some(provider) => vec![provider],
            None if snapshot.transitive_peer_dependencies
                .as_ref()
                .is_some_and(|names| {
                    names
                        .iter()
                        .any(|candidate| candidate == name)
                }) =>
            {
                hashed_suffix::transitive_peer_keys(self.lockfile, snapshot, name, self.bound_cache)
            }
            None => Vec::new(),
        };
        providers
            .into_iter()
            .filter(|provider| provider.without_peer() == peer.without_peer())
            .filter_map(|provider| provider_with_peers(self.lockfile, provider))
            .collect()
    }
}

/// A provider whose own key carries peers, with its snapshot, so its peers
/// are read too.
fn provider_with_peers(
    lockfile: &pnpm_lockfile::Lockfile,
    provider: pnpm_lockfile::PkgNameVerPeer,
) -> Option<(pnpm_lockfile::PkgNameVerPeer, Option<&pnpm_lockfile::SnapshotEntry>)> {
    if provider.suffix.peer().is_empty() {
        return None;
    }
    let snapshot = lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&provider));
    Some((provider, snapshot))
}

/// The peers `key`'s own suffix pins, each with the snapshot key of its
/// provider.
fn locked_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    bound_cache: &mut hashed_suffix::BoundCache,
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    let metadata = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()));
    let recorded = snapshot
        // Transitive aliases are recorded on descendants, not this snapshot's edges.
        .filter(|snapshot| snapshot.transitive_peer_dependencies.as_ref().is_none_or(Vec::is_empty))
        .and_then(|snapshot| snapshot.recorded_peer_bindings.as_ref())
        .and_then(|bindings| bindings.peer_keys_for(key));
    let peers = match recorded {
        Some(peers) => peers.to_vec(),
        None => match PeerBindings::from_key(key) {
            None => return Vec::new(),
            Some(PeerBindings::Hashed) => {
                return hashed_peer_keys(lockfile, key, snapshot, metadata, bound_cache);
            }
            Some(bindings) => bindings.providers().collect(),
        },
    };
    if peers.is_empty() {
        return Vec::new();
    }
    let mut explicit: Vec<_> = peers
        .iter()
        .map(|peer| (peer.name.to_string(), peer.suffix.without_peer().to_string()))
        .collect();
    if let Some(snapshot) = snapshot {
        restore_aliased_peer_names(snapshot, metadata, &mut explicit);
    }
    explicit
        .into_iter()
        .map(|(name, _)| name)
        .zip(peers)
        .collect()
}

/// A cycle can truncate a suffix provider to its name and version. Its
/// dependency edge still names the snapshot carrying its own peer bindings.
fn snapshot_peer_provider(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    name: &str,
    peer: &pnpm_lockfile::PkgNameVerPeer,
) -> Option<pnpm_lockfile::PkgNameVerPeer> {
    if !peer.suffix.peer().is_empty() {
        return None;
    }
    let alias: PkgName = name.parse().ok()?;
    let reference = snapshot.dependencies
        .as_ref()
        .and_then(|deps| deps.get(&alias))
        .or_else(|| snapshot.optional_dependencies.as_ref()?.get(&alias))?;
    let provider = reference.resolve(&alias)?;
    (provider.without_peer() == peer.without_peer()).then_some(provider)
}

/// Rename the suffix segments an npm alias provides back to the name
/// the provider is installed under.
///
/// A peer suffix names each provider by the package it resolved to,
/// while peer resolution keys providers by the name they occupy in the
/// dependent's `node_modules` — which for `"peer": "npm:provider@1"` is
/// `peer`, not `provider`. Reading the segment back verbatim would pin a
/// peer nobody declares and leave the declared one unpinned, so a
/// repeated resolution is free to pick the other variant.
pub(super) fn restore_aliased_peer_names(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
    peers: &mut [(String, String)],
) {
    let mut providers = provider_aliases(snapshot, metadata);
    if providers.is_empty() {
        return;
    }
    for (name, version) in peers.iter_mut() {
        let Some(alias) = providers
            .get_mut(&format!("{name}@{version}"))
            .and_then(ProviderAliases::claim)
        else {
            continue;
        };
        *name = alias;
    }
}

/// Index the snapshot's dependency edges by the `name@version` of the
/// package each one installs, so every suffix segment is attributed in
/// one lookup rather than another scan of the edges.
///
/// Empty — and every segment therefore left alone — unless some edge
/// installs a package under a name other than its own, since that is the
/// only shape that makes a segment disagree with its edge.
pub(super) fn provider_aliases(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
) -> HashMap<String, ProviderAliases> {
    if !dependency_edges(snapshot)
        .any(|(_, reference)| matches!(reference, pnpm_lockfile::SnapshotDepRef::Alias(_)))
    {
        return HashMap::default();
    }
    let declared = metadata.and_then(|metadata| metadata.peer_dependencies.as_ref());
    let mut providers = HashMap::<String, ProviderAliases>::default();
    for (edge_name, reference) in dependency_edges(snapshot) {
        let (provider, ver_peer) = match reference {
            pnpm_lockfile::SnapshotDepRef::Plain(ver_peer) => (edge_name.to_string(), ver_peer),
            pnpm_lockfile::SnapshotDepRef::Alias(target) => {
                (target.name.to_string(), &target.suffix)
            }
            pnpm_lockfile::SnapshotDepRef::Link(_) => continue,
        };
        let edge_name = edge_name.to_string();
        let declares_peer = declared.is_some_and(|peers| peers.contains_key(&edge_name));
        providers
            .entry(format!("{provider}@{}", ver_peer.without_peer()))
            .or_default()
            .record(edge_name, declares_peer);
    }
    providers
}

/// The names one provider is installed under, split by whether the
/// dependent declares that name as a peer.
///
/// The suffix spells one segment per peer-resolved edge and never merges
/// equal ones, so each segment claims an edge of its own.
#[derive(Default)]
pub(super) struct ProviderAliases {
    pub(super) declared_peers: Vec<String>,
    /// How many of `declared_peers` the segments seen so far claimed.
    pub(super) claimed: usize,
    pub(super) ordinary: Option<String>,
    pub(super) ordinaries: usize,
}

impl ProviderAliases {
    pub(super) fn record(&mut self, alias: String, declares_peer: bool) {
        if declares_peer {
            self.declared_peers.push(alias);
            return;
        }
        self.ordinaries += 1;
        if self.ordinaries == 1 {
            self.ordinary = Some(alias);
        }
    }

    /// The name to attribute the next segment naming this provider to.
    ///
    /// Declared peer edges go first, matching the
    /// `peerDependencies`-keyed lookup the TypeScript CLI restores peer
    /// context with; an ordinary edge takes the segment left over, which
    /// is how a peer propagated up from a child — satisfied by an
    /// ordinary dependency, under the name the child declared — gets its
    /// name back.
    ///
    /// Competing ordinary edges are unattributable, since a dependency
    /// may be aliased onto the very package and version a peer resolved
    /// to and nothing in the lockfile tells the two apart, so the segment
    /// keeps the name the suffix spelled.
    pub(super) fn claim(&mut self) -> Option<String> {
        if self.claimed < self.declared_peers.len() {
            self.claimed += 1;
            return Some(self.declared_peers[self.claimed - 1].clone());
        }
        if self.ordinaries == 1 {
            return self.ordinary.take();
        }
        None
    }
}

pub(super) fn dependency_edges(
    snapshot: &pnpm_lockfile::SnapshotEntry,
) -> impl Iterator<Item = (&PkgName, &pnpm_lockfile::SnapshotDepRef)> {
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
}

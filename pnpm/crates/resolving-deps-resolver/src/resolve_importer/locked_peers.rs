use super::{Arc, DependencyGroup, HashMap, HashSet, TreeCtx};
use hashed_suffix::hashed_peer_keys;
use pnpm_lockfile::{PeerBindings, PkgName};

mod hashed_suffix;

/// The peer versions the prior lockfile locked for the importer.
pub(super) struct LockedPeers {
    pub(super) versions: Arc<HashMap<String, HashSet<String>>>,
}

impl LockedPeers {
    pub(super) fn of(ctx: &TreeCtx, importer_id: &str) -> Self {
        let versions = Arc::new(importer_locked_peer_versions(
            ctx.workspace()
                .wanted_lockfile()
                .map(AsRef::as_ref),
            importer_id,
        ));
        Self { versions }
    }
}

pub(super) fn importer_locked_peer_versions(
    wanted_lockfile: Option<&pnpm_lockfile::Lockfile>,
    importer_id: &str,
) -> HashMap<String, HashSet<String>> {
    let Some(lockfile) = wanted_lockfile else {
        return HashMap::default();
    };
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return all_locked_peer_versions(lockfile);
    };
    let mut versions = HashMap::<String, HashSet<String>>::default();
    for (alias, dependency) in importer.dependencies_by_groups([
        DependencyGroup::Prod,
        DependencyGroup::Optional,
        DependencyGroup::Dev,
    ]) {
        let Some(key) = dependency.version.resolved_key(alias) else {
            continue;
        };
        let snapshot = lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&key));
        for (name, version) in locked_peer_versions_for_key(lockfile, &key, snapshot) {
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
    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        for (name, version) in locked_peer_versions_for_key(lockfile, key, Some(snapshot)) {
            versions
                .entry(name)
                .or_default()
                .insert(version);
        }
    }
    versions
}

/// The peer name/version pairs the wanted lockfile pinned for `key`,
/// including the peers of every peer provider the suffix names.
///
/// An explicit suffix already spells them out, save for the names an
/// npm alias renamed ([`restore_aliased_peer_names`]). A hashed suffix
/// spells out nothing, so the pairs are recovered from the snapshots
/// ([`hashed_peer_keys`]). Both spellings of one key must yield the same
/// pairs: `dedupe` rewrites a long suffix between them, and pins that
/// depend on the spelling make it flip between two lockfiles.
/// Nested suffixes need their own provider's snapshot: the outer
/// snapshot does not name that provider's aliased peers.
pub(super) fn locked_peer_versions_for_key(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
) -> Vec<(String, String)> {
    let mut versions = Vec::new();
    let mut visited = HashSet::default();
    let mut pending = vec![(key.clone(), snapshot)];
    while let Some((key, snapshot)) = pending.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }
        for (name, peer) in locked_peer_keys(lockfile, &key, snapshot) {
            versions.push((name, peer.suffix.without_peer().to_string()));
            pending.extend(provider_with_peers(lockfile, peer));
        }
    }
    versions
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
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    let metadata = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()));
    let peers: Vec<pnpm_lockfile::PkgNameVerPeer> = match PeerBindings::from_key(key) {
        None => return Vec::new(),
        Some(PeerBindings::Hashed) => return hashed_peer_keys(lockfile, snapshot, metadata),
        Some(bindings) => bindings.providers().collect(),
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

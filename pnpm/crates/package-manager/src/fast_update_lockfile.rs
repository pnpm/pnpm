use pnpm_lockfile::{
    Lockfile, PkgName, PkgNameVerPeer, Prefix, ResolvedDependencySpec, SnapshotDepRef,
    SnapshotEntry, VersionPart,
};
use std::collections::{HashSet, VecDeque};

/// What the fast-update handlers did to the dependency graph, so the
/// maintenance that keeps the lockfile consistent — pruning, the
/// peer-suffix safety check, catalog entry pruning, and the `optional`
/// flag recompute — runs once over the combined result instead of once
/// per handler.
#[derive(Default)]
pub(crate) struct GraphEdits {
    /// What the severed importer or snapshot edges pointed at.
    pub(crate) dropped: DroppedEdges,
    /// Whether an edge was added or moved into or out of
    /// `optionalDependencies`, which changes the reachability-derived
    /// `optional` flags for a subtree.
    pub(crate) optional_flags_are_stale: bool,
}

/// What the edges a fast update severed pointed at, each written the way
/// a peer suffix names a package: `name@version`, or a bare `name@` when
/// the reference pins no version a suffix segment can be compared
/// against — a link or a tarball, whose suffix segment carries the
/// resolved manifest version instead — which makes every suffix naming
/// that name suspect.
#[derive(Default)]
pub(crate) struct DroppedEdges(HashSet<String>);

/// The `snapshots:` key a dependency record points at, across the
/// importer-level and snapshot-level reference shapes, so a severed edge
/// can be recorded from either.
pub(crate) trait DroppedEdgeTarget {
    fn resolved_key(&self, alias: &PkgName) -> Option<PkgNameVerPeer>;
}

impl DroppedEdgeTarget for ResolvedDependencySpec {
    fn resolved_key(&self, alias: &PkgName) -> Option<PkgNameVerPeer> {
        self.version.resolved_key(alias)
    }
}

impl DroppedEdgeTarget for SnapshotDepRef {
    fn resolved_key(&self, alias: &PkgName) -> Option<PkgNameVerPeer> {
        self.resolve(alias)
    }
}

impl DroppedEdges {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Record the edge from `alias` to `target` as severed.
    pub(crate) fn record(&mut self, alias: &PkgName, target: &impl DroppedEdgeTarget) {
        let peer_id = match target.resolved_key(alias) {
            // A peer suffix names an aliased dependency by the package's
            // own name, not by the name it is linked into `node_modules`
            // under; a link has no key to take a name from at all.
            Some(key) => peer_id_of(&key).unwrap_or_else(|| format!("{}@", key.name)),
            None => format!("{alias}@"),
        };
        self.0.insert(peer_id);
    }

    /// Whether `peers` — the peer suffix of a snapshot key — names none
    /// of the severed edges' targets, at any nesting depth. Without
    /// `dedupePeers` a peer is named by its whole dep path, and the peers
    /// that path pins are as much a part of the dependent's key as the
    /// top-level ones. A `patch_hash=` segment names no package; a suffix
    /// pnpm shortened into a hash names nothing that can be ruled out.
    fn are_absent_from(&self, peers: &str) -> bool {
        peers
            .split(['(', ')'])
            .filter(|peer_id| !peer_id.is_empty() && !peer_id.starts_with("patch_hash="))
            .all(|peer_id| {
                // The `@` of a scoped name is not the separator, and a
                // segment the lockfile put a multi-byte character in front
                // of must not be sliced blindly.
                let Some(separator) =
                    peer_id.match_indices('@').map(|(index, _)| index).find(|index| *index > 0)
                else {
                    return false;
                };
                !self.0.contains(peer_id) && !self.0.contains(&peer_id[..=separator])
            })
    }
}

/// The id a peer suffix names `key` by.
fn peer_id_of(key: &PkgNameVerPeer) -> Option<String> {
    if key.suffix.prefix() != Prefix::None {
        return None;
    }
    match key.suffix.version() {
        VersionPart::Semver(version) => Some(format!("{}@{version}", key.name)),
        VersionPart::RegistryQualified { registry_name, version } => {
            Some(format!("{}@{registry_name}:{version}", key.name))
        }
        VersionPart::File(_) | VersionPart::NonSemver(_) => None,
    }
}

/// Settle the graph after every handler has run: drop what nothing
/// reaches any more, refuse the update when a surviving peer suffix
/// embeds a dropped package (its key would need a rewrite, not a prune),
/// drop catalog entries with no referent left, and recompute the
/// `optional` flags. `false` leaves the caller on the full-resolution
/// path.
pub(crate) fn finish_graph_edits(candidate: &mut Lockfile, edits: &GraphEdits) -> bool {
    if !edits.dropped.is_empty() {
        prune_unreachable_packages(candidate);
        if !peer_suffixes_are_independent_of(candidate, &edits.dropped) {
            return false;
        }
        prune_unreferenced_catalog_entries(candidate);
    }
    if !edits.dropped.is_empty() || edits.optional_flags_are_stale {
        recompute_optional_flags(candidate);
    }
    true
}

/// Whether no surviving snapshot resolves a peer through one of `dropped`.
///
/// A dropped package that some snapshot reaches as a peer is embedded in
/// that snapshot's key, so removing it would rekey the dependent rather
/// than only prune. A package the same alias still provides at another
/// version is no such peer: the suffix names the version, not the alias.
fn peer_suffixes_are_independent_of(lockfile: &Lockfile, dropped: &DroppedEdges) -> bool {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return true;
    };
    snapshots.keys().all(|key| dropped.are_absent_from(key.suffix.peer()))
}

pub(crate) fn prune_unreachable_packages(lockfile: &mut Lockfile) {
    let Some(reachable) = reachable_keys(lockfile) else { return };
    let reachable_metadata: HashSet<_> =
        reachable.iter().map(PkgNameVerPeer::without_peer).collect();
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        snapshots.retain(|key, _| reachable.contains(key));
        if snapshots.is_empty() {
            lockfile.snapshots = None;
        }
    }
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| reachable_metadata.contains(key));
        if packages.is_empty() {
            lockfile.packages = None;
        }
    }
}

/// Every snapshot key an importer still reaches. `None` when the lockfile
/// records no snapshots at all.
fn reachable_keys(lockfile: &Lockfile) -> Option<HashSet<PkgNameVerPeer>> {
    let snapshots = lockfile.snapshots.as_ref()?;
    let mut reachable = HashSet::new();
    let mut queue = importer_root_keys(lockfile);
    while let Some(key) = queue.pop_front() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        queue.extend(snapshot_child_keys(snapshot));
    }
    Some(reachable)
}

/// The snapshot keys every importer declares directly.
fn importer_root_keys(lockfile: &Lockfile) -> VecDeque<PkgNameVerPeer> {
    lockfile
        .importers
        .values()
        .flat_map(|importer| {
            [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            .flatten()
        })
        .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
        .collect()
}

fn snapshot_child_keys(snapshot: &SnapshotEntry) -> impl Iterator<Item = PkgNameVerPeer> + '_ {
    [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
}

/// Recompute every snapshot's `optional` flag from what still reaches it:
/// set when every path from any importer goes through an
/// `optionalDependencies` edge, cleared otherwise. An importer edge that
/// moves into or out of `optionalDependencies`, or a removal that severs
/// the last non-optional path, changes the flag for the whole subtree.
pub(crate) fn recompute_optional_flags(lockfile: &mut Lockfile) {
    let Some(only_optionally_reached) = optionally_reached_keys(lockfile) else { return };
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        for (key, snapshot) in snapshots.iter_mut() {
            snapshot.optional = only_optionally_reached.contains(key);
        }
    }
}

/// The snapshot keys no non-optional path reaches. `None` when the lockfile
/// records no snapshots at all.
fn optionally_reached_keys(lockfile: &Lockfile) -> Option<HashSet<PkgNameVerPeer>> {
    let snapshots = lockfile.snapshots.as_ref()?;
    // Walk `(key, reached-optionally)`: `dependencies` edges keep the
    // context, `optionalDependencies` edges always enter it.
    let mut visited = HashSet::new();
    let mut queue = importer_optional_roots(lockfile);
    while let Some((key, optional)) = queue.pop_front() {
        if !visited.insert((key.clone(), optional)) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        queue.extend(snapshot_optional_children(snapshot, optional));
    }
    let non_optional: HashSet<_> =
        visited.iter().filter_map(|(key, optional)| (!optional).then_some(key.clone())).collect();
    Some(
        visited
            .into_iter()
            .filter_map(|(key, optional)| (optional && !non_optional.contains(&key)).then_some(key))
            .collect(),
    )
}

fn importer_optional_roots(lockfile: &Lockfile) -> VecDeque<(PkgNameVerPeer, bool)> {
    lockfile
        .importers
        .values()
        .flat_map(|importer| {
            [
                (importer.dependencies.as_ref(), false),
                (importer.dev_dependencies.as_ref(), false),
                (importer.optional_dependencies.as_ref(), true),
            ]
            .into_iter()
            .flat_map(|(dependencies, optional)| {
                dependencies.into_iter().flatten().map(move |entry| (entry, optional))
            })
        })
        .filter_map(|((alias, spec), optional)| {
            spec.version.resolved_key(alias).map(|key| (key, optional))
        })
        .collect()
}

fn snapshot_optional_children(
    snapshot: &SnapshotEntry,
    optional: bool,
) -> impl Iterator<Item = (PkgNameVerPeer, bool)> + '_ {
    [(snapshot.dependencies.as_ref(), optional), (snapshot.optional_dependencies.as_ref(), true)]
        .into_iter()
        .flat_map(|(dependencies, next_optional)| {
            dependencies.into_iter().flatten().map(move |entry| (entry, next_optional))
        })
        .filter_map(|((alias, dep_ref), next_optional)| {
            dep_ref.resolve(alias).map(|key| (key, next_optional))
        })
}

/// Drop every catalog snapshot entry that no importer references any
/// more, matching what a full resolution records after the same removal.
pub(crate) fn prune_unreferenced_catalog_entries(lockfile: &mut Lockfile) {
    let Some(catalogs) = lockfile.catalogs.as_ref() else {
        return;
    };
    let stale: Vec<(String, String)> = catalogs
        .iter()
        .flat_map(|(catalog_name, entries)| {
            entries.keys().map(move |alias| (catalog_name.clone(), alias.clone()))
        })
        .filter(|(catalog_name, alias)| {
            !crate::fast_update_catalogs::catalog_entry_is_referenced(lockfile, catalog_name, alias)
        })
        .collect();
    if stale.is_empty() {
        return;
    }
    let catalogs = lockfile.catalogs.as_mut().expect("checked above");
    for (catalog_name, alias) in stale {
        if let Some(entries) = catalogs.get_mut(&catalog_name) {
            entries.remove(&alias);
            if entries.is_empty() {
                catalogs.remove(&catalog_name);
            }
        }
    }
    if catalogs.is_empty() {
        lockfile.catalogs = None;
    }
}

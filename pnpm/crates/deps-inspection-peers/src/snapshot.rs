use super::{
    HashMap, HashSet, PackageMetadata, ParentPkg, Path, PeerIssues, PkgName, PkgNameVerPeer,
    SnapshotDepRef, SnapshotEntry, get_peer_version_range, record_bad_peer, record_missing_peer,
    resolve_link_version,
};

pub(super) fn walk_snapshot(
    initial_keys: Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &Path,
    visited: &mut HashSet<PkgNameVerPeer>,
    issues: &mut PeerIssues,
) {
    let mut stack = initial_keys;

    while let Some((key, parents)) = stack.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let mut current_parents = parents;
        current_parents.push(ParentPkg {
            name: key.name.to_string(),
            version: get_pkg_version(&key, packages),
        });

        let snapshot = snapshots.get(&key);
        check_snapshot_peers(SnapshotPeers {
            key: &key,
            snapshot,
            packages,
            lockfile_dir,
            parents: &current_parents,
            issues,
        });

        stack.extend(child_keys(snapshot).map(|child| (child, current_parents.clone())));
    }
}

/// One walked package: its own snapshot, and the parent chain that reached
/// it.
pub(super) struct SnapshotPeers<'a> {
    key: &'a PkgNameVerPeer,
    snapshot: Option<&'a SnapshotEntry>,
    packages: &'a HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &'a Path,
    parents: &'a [ParentPkg],
    issues: &'a mut PeerIssues,
}

/// Record every peer dependency the package declares that its snapshot
/// leaves unsatisfied.
pub(super) fn check_snapshot_peers(inputs: SnapshotPeers<'_>) {
    let issues = inputs.issues;
    let Some(meta) = inputs.packages.get(&inputs.key.without_peer()) else {
        return;
    };
    let Some(peers) = &meta.peer_dependencies else {
        return;
    };

    for (peer_name, peer_range) in peers {
        let peer_range = get_peer_version_range(peer_range);
        let optional = meta.peer_dependencies_meta
            .as_ref()
            .and_then(|meta_map| meta_map.get(peer_name))
            .is_some_and(|peer_meta| peer_meta.optional);

        let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else {
            continue;
        };
        let dep_ref = inputs.snapshot.and_then(|entry| snapshot_dependency(entry, &peer_pkg_name));
        let Some(dep_ref) = dep_ref else {
            record_missing_peer(issues, peer_name, inputs.parents, optional, &peer_range);
            continue;
        };

        let Some(found_version) = resolved_snapshot_version(dep_ref, inputs.lockfile_dir) else {
            continue;
        };
        record_bad_peer(
            issues,
            peer_name,
            inputs.parents,
            optional,
            &peer_range,
            found_version,
        );
    }
}

pub(super) fn snapshot_dependency<'a>(
    snapshot: &'a SnapshotEntry,
    name: &PkgName,
) -> Option<&'a SnapshotDepRef> {
    snapshot.dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| {
            snapshot.optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
}

/// The version a snapshot dependency reference resolves to. A reference that
/// is neither a registry version nor a link has no version to check against.
pub(super) fn resolved_snapshot_version(
    dep_ref: &SnapshotDepRef,
    lockfile_dir: &Path,
) -> Option<String> {
    if let Some(ver_peer) = dep_ref.ver_peer() {
        return Some(ver_peer.version().to_string());
    }
    let link_target = dep_ref.as_link_target()?;
    Some(
        resolve_link_version(lockfile_dir, lockfile_dir, link_target)
            .unwrap_or_else(|| format!("link:{link_target}")),
    )
}

pub(super) fn child_keys(
    snapshot: Option<&SnapshotEntry>,
) -> impl Iterator<Item = PkgNameVerPeer> + '_ {
    snapshot
        .into_iter()
        .flat_map(|snapshot| {
            snapshot.dependencies
                .iter()
                .flat_map(|deps| deps.iter())
                .chain(
                    snapshot.optional_dependencies
                        .iter()
                        .flat_map(|deps| deps.iter()),
                )
        })
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
}

pub(super) fn get_pkg_version(
    key: &PkgNameVerPeer,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
) -> String {
    let base_key = key.without_peer();
    packages
        .get(&base_key)
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| key.suffix.version().to_string())
}

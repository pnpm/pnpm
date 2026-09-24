use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use node_semver::{Range, Version};
use pnpm_lockfile::{
    PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer, SnapshotDepRef, SnapshotEntry,
};
use pnpm_resolving_resolver_base::get_peer_version_range;

use crate::{
    ParentPkg, PeerIssues,
    linked::{record_bad_peer, record_missing_peer, resolve_link_version},
    ranges::{parse_range_to_intervals, preprocess_hyphen_ranges},
};

pub(super) fn walk_snapshot(
    initial_keys: Vec<PkgNameVerPeer>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &Path,
    visited: &mut HashSet<PkgNameVerPeer>,
    issues: &mut PeerIssues,
) {
    let mut stack: Vec<(PkgNameVerPeer, Vec<ParentPkg>)> = initial_keys
        .into_iter()
        .map(|key| (key, Vec::new()))
        .collect();

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
struct SnapshotPeers<'a> {
    key: &'a PkgNameVerPeer,
    snapshot: Option<&'a SnapshotEntry>,
    packages: &'a HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &'a Path,
    parents: &'a [ParentPkg],
    issues: &'a mut PeerIssues,
}

/// Record every peer dependency the package declares that its snapshot
/// leaves unsatisfied.
fn check_snapshot_peers(inputs: SnapshotPeers<'_>) {
    let issues = inputs.issues;
    let Some(meta) = inputs.packages.get(&inputs.key.without_peer()) else { return };
    let Some(peers) = &meta.peer_dependencies else { return };

    for (peer_name, peer_range) in peers {
        let peer_range = get_peer_version_range(peer_range);
        let optional = meta.peer_dependencies_meta
            .as_ref()
            .and_then(|meta_map| meta_map.get(peer_name))
            .is_some_and(|peer_meta| peer_meta.optional);

        let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
        let dep_ref = inputs.snapshot.and_then(|entry| snapshot_dependency(entry, &peer_pkg_name));
        let Some(dep_ref) = dep_ref else {
            record_missing_peer(issues, peer_name, inputs.parents, optional, &peer_range);
            continue;
        };

        let Some(found_version) = resolved_snapshot_version(
            dep_ref,
            &peer_pkg_name,
            inputs.packages,
            inputs.lockfile_dir,
        ) else {
            continue;
        };
        record_bad_peer(issues, peer_name, inputs.parents, optional, &peer_range, found_version);
    }
}

fn snapshot_dependency<'a>(
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

/// Resolve a snapshot peer's package version, preferring package metadata.
pub(crate) fn resolved_snapshot_version(
    dep_ref: &SnapshotDepRef,
    peer_name: &PkgName,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &Path,
) -> Option<String> {
    if let Some(key) = dep_ref.resolve(peer_name) {
        return Some(get_pkg_version(&key, packages));
    }
    let link_target = dep_ref.as_link_target()?;
    Some(
        resolve_link_version(lockfile_dir, lockfile_dir, link_target)
            .unwrap_or_else(|| format!("link:{link_target}")),
    )
}

fn child_keys(snapshot: Option<&SnapshotEntry>) -> impl Iterator<Item = PkgNameVerPeer> + '_ {
    snapshot
        .into_iter()
        .flat_map(|snapshot| {
            snapshot.dependencies
                .iter()
                .flat_map(|deps| deps.iter())
                .chain(snapshot.optional_dependencies.iter().flat_map(|deps| deps.iter()))
        })
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
}

fn get_pkg_version(
    key: &PkgNameVerPeer,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
) -> String {
    let base_key = key.without_peer();
    packages
        .get(&base_key)
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| extract_peer_version(&key.suffix))
}

pub(super) fn extract_peer_version(ver_peer: &PkgVerPeer) -> String {
    ver_peer
        .registry_qualified()
        .map_or_else(|| ver_peer.version().to_string(), |(_, version)| version.to_string())
}

pub(super) fn satisfies(version: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    let Ok(parsed_version) = Version::parse(version) else {
        return version == range;
    };
    let Ok(parsed_range) = Range::parse(range) else {
        return version == range;
    };
    if parsed_version.satisfies(&parsed_range) {
        return true;
    }
    if !parsed_version.is_prerelease() {
        return false;
    }
    // pnpm asks semver for `includePrerelease`, which drops the rule
    // that a prerelease only satisfies a comparator carrying a
    // prerelease of its own `major.minor.patch` — `node-semver`'s Rust
    // port applies that rule unconditionally. What is left is the plain
    // bound check, and ordering still holds: `18.3.0-canary` satisfies
    // `^18.0.0`, while `2.0.0-beta.1` stays below `>=2.0.0`.
    parse_range_to_intervals(&preprocess_hyphen_ranges(range))
        .is_some_and(|intervals| {
            intervals
                .iter()
                .any(|interval| interval.contains(&parsed_version))
        })
}

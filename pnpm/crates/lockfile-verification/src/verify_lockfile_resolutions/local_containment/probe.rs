use std::collections::{
    BTreeMap,
    BTreeSet,
    HashSet,
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    SnapshotDepRef,
    VersionPart,
    is_non_local_filesystem_subdep_spec,
    package_root_link_target,
};

use super::{
    super::{
        local_git_shape::{
            has_local_git_resolution,
            is_local_git_package,
        },
        offline_checks::has_local_filesystem_resolution,
    },
    LocalResolutionPolicy,
    policy::parent_semver_or_version,
    roots::{
        AuthorizedLocalRoots,
        is_snapshot_backed_by_local_resolution,
    },
};

/// Non-local package identity and local-candidate dependency aliases to probe
/// for pristine manifest metadata or `readPackage` hook rewrites.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalResolutionProbeParent {
    pub pkg_key: PkgNameVerPeer,
    pub resolution: Option<LockfileResolution>,
    pub name: String,
    pub version: Option<String>,
    pub dep_aliases: Vec<String>,
    pub optional_aliases: Vec<String>,
    pub peer_aliases: Vec<String>,
}

struct CandidateAccumulator {
    pkg_key: PkgNameVerPeer,
    resolution: Option<LockfileResolution>,
    name: String,
    version: Option<String>,
    deps: BTreeSet<String>,
    opts: BTreeSet<String>,
    peers: BTreeSet<String>,
}

/// Collect distinct non-local package sources in `lockfile` whose snapshots or
/// metadata carry local-filesystem child or peer edges.
#[must_use]
pub fn local_resolution_candidate_parents(lockfile: &Lockfile) -> Vec<LocalResolutionProbeParent> {
    local_resolution_candidate_parents_for_policy(lockfile, &LocalResolutionPolicy::default())
}

/// Like [`local_resolution_candidate_parents`], restricted to snapshots and
/// packages reachable from `policy.scope.selected_importer_ids` when set.
#[must_use]
pub fn local_resolution_candidate_parents_for_policy(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
) -> Vec<LocalResolutionProbeParent> {
    let reachability = policy.scope.selected_importer_ids
        .as_ref()
        .map(|_| {
            let roots = AuthorizedLocalRoots::collect(lockfile, policy);
            let snaps: HashSet<PkgNameVerPeer> = roots.importers
                .iter()
                .flat_map(|imp| imp.reachable_snapshots.iter().cloned())
                .collect();
            let pkgs: HashSet<PkgNameVerPeer> = roots.importers
                .iter()
                .flat_map(|imp| imp.reachable_packages.iter().cloned())
                .collect();
            (snaps, pkgs)
        });
    let mut grouped: BTreeMap<String, CandidateAccumulator> = BTreeMap::new();
    collect_snapshot_probe_candidates(
        lockfile,
        reachability.as_ref().map(|(snaps, _)| snaps),
        &mut grouped,
    );
    collect_peer_probe_candidates(
        lockfile,
        reachability.as_ref().map(|(_, pkgs)| pkgs),
        &mut grouped,
    );
    grouped
        .into_values()
        .map(|acc| LocalResolutionProbeParent {
            pkg_key: acc.pkg_key,
            resolution: acc.resolution,
            name: acc.name,
            version: acc.version,
            dep_aliases: acc.deps.into_iter().collect(),
            optional_aliases: acc.opts.into_iter().collect(),
            peer_aliases: acc.peers.into_iter().collect(),
        })
        .collect()
}

fn collect_snapshot_probe_candidates(
    lockfile: &Lockfile,
    reachable_snaps: Option<&HashSet<PkgNameVerPeer>>,
    grouped: &mut BTreeMap<String, CandidateAccumulator>,
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return;
    };
    for (snap_key, snapshot) in snapshots {
        if reachable_snaps.is_some_and(|set| !set.contains(snap_key)) {
            continue;
        }
        record_snapshot_probe_candidate(lockfile, snap_key, snapshot, grouped);
    }
}

fn record_snapshot_probe_candidate(
    lockfile: &Lockfile,
    snap_key: &PkgNameVerPeer,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    grouped: &mut BTreeMap<String, CandidateAccumulator>,
) {
    let base_snap = snap_key.without_peer();
    let snap_meta = lockfile.packages
        .as_ref()
        .and_then(|pkgs| pkgs.get(&base_snap));
    if !is_eligible_probe_parent(&base_snap, snap_meta) {
        return;
    }
    let deps = local_child_aliases(lockfile, snapshot.dependencies.as_ref());
    let opts = local_child_aliases(lockfile, snapshot.optional_dependencies.as_ref());
    if deps.is_empty() && opts.is_empty() {
        return;
    }
    let entry = grouped
        .entry(base_snap.to_string())
        .or_insert_with(|| CandidateAccumulator {
            name: base_snap.name.to_string(),
            version: parent_semver_or_version(&base_snap, snap_meta),
            pkg_key: base_snap,
            resolution: snap_meta.map(|meta| meta.resolution.clone()),
            deps: BTreeSet::new(),
            opts: BTreeSet::new(),
            peers: BTreeSet::new(),
        });
    entry.deps.extend(deps);
    entry.opts.extend(opts);
}

fn collect_peer_probe_candidates(
    lockfile: &Lockfile,
    reachable_pkgs: Option<&HashSet<PkgNameVerPeer>>,
    grouped: &mut BTreeMap<String, CandidateAccumulator>,
) {
    let Some(packages) = lockfile.packages.as_ref() else {
        return;
    };
    for (pkg_key, meta) in packages {
        if reachable_pkgs.is_some_and(|set| !set.contains(pkg_key)) {
            continue;
        }
        if !is_eligible_probe_parent(pkg_key, Some(meta)) {
            continue;
        }
        let peers = local_peer_aliases(meta);
        if peers.is_empty() {
            continue;
        }
        grouped
            .entry(pkg_key.to_string())
            .or_insert_with(|| CandidateAccumulator {
                name: pkg_key.name.to_string(),
                version: parent_semver_or_version(pkg_key, Some(meta)),
                pkg_key: pkg_key.clone(),
                resolution: Some(meta.resolution.clone()),
                deps: BTreeSet::new(),
                opts: BTreeSet::new(),
                peers: BTreeSet::new(),
            })
            .peers
            .extend(peers);
    }
}

fn is_eligible_probe_parent(base_key: &PkgNameVerPeer, meta: Option<&PackageMetadata>) -> bool {
    if is_snapshot_backed_by_local_resolution(base_key, meta)
        || is_local_git_package(base_key, meta)
    {
        return false;
    }
    !meta.is_some_and(|entry| {
        has_local_filesystem_resolution(&entry.resolution)
            || has_local_git_resolution(&entry.resolution)
    })
}

fn local_child_aliases(
    lockfile: &Lockfile,
    deps: Option<&std::collections::HashMap<PkgName, SnapshotDepRef>>,
) -> Vec<String> {
    let Some(deps) = deps else {
        return Vec::new();
    };
    let packages = lockfile.packages.as_ref();
    deps.iter()
        .filter(|(alias, dep_ref)| is_local_dep_ref(packages, alias, dep_ref))
        .map(|(alias, _)| alias.to_string())
        .collect()
}

fn is_local_dep_ref(
    packages: Option<&std::collections::HashMap<PkgNameVerPeer, PackageMetadata>>,
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
) -> bool {
    if let SnapshotDepRef::Link(target) = dep_ref {
        return package_root_link_target(target).is_none();
    }
    let Some(child_key) = dep_ref.resolve(alias) else {
        return false;
    };
    let base_key = child_key.without_peer();
    let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
    matches!(base_key.suffix.version(), VersionPart::File(_))
        || is_local_git_package(&base_key, meta)
        || meta.is_some_and(|entry| has_local_filesystem_resolution(&entry.resolution))
}

fn local_peer_aliases(meta: &PackageMetadata) -> Vec<String> {
    meta.peer_dependencies
        .iter()
        .flat_map(|peers| peers.iter())
        .filter(|(_, spec)| {
            spec.strip_prefix("link:")
                .and_then(package_root_link_target)
                .is_none()
                && is_non_local_filesystem_subdep_spec(spec, |_| false)
        })
        .map(|(alias, _)| alias.clone())
        .collect()
}

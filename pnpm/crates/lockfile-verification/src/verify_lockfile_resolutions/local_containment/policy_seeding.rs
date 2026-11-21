use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    SnapshotDepRef,
    SnapshotEntry,
    VersionPart,
};

use super::{
    super::{
        local_git_shape::is_local_git_package,
        offline_checks::has_local_filesystem_resolution,
    },
    LocalDepField,
    policy::policy_authorizes_package,
    roots::{
        is_snapshot_backed_by_local_resolution,
        normalize_rel_dir,
    },
    snapshot_scan::{
        PolicyClosure,
        ScopedScan,
    },
};

pub(super) fn seed_reachable_policy_closures(
    scan: &mut ScopedScan<'_>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
) {
    let packages = scan.lockfile.packages.as_ref();
    for snap_key in reachable_snapshots {
        let Some(snapshot) = snapshots.get(snap_key) else {
            continue;
        };
        let base_snap = snap_key.without_peer();
        let snap_meta = packages.and_then(|pkgs| pkgs.get(&base_snap));
        for (field, deps) in [
            (LocalDepField::Dependencies, &snapshot.dependencies),
            (LocalDepField::OptionalDependencies, &snapshot.optional_dependencies),
        ] {
            for (alias, dep_ref) in deps.iter().flatten() {
                seed_policy_package_child(
                    scan,
                    reachable_snapshots,
                    (snap_key, snap_meta),
                    (field, alias, dep_ref),
                );
            }
        }
    }
}

fn seed_policy_package_child(
    scan: &mut ScopedScan<'_>,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    (parent_key, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    (field, alias, dep_ref): (LocalDepField, &PkgName, &SnapshotDepRef),
) {
    let Some(child_key) = dep_ref.resolve(alias) else {
        return;
    };
    let base_key = child_key.without_peer();
    let meta = scan.lockfile.packages
        .as_ref()
        .and_then(|pkgs| pkgs.get(&base_key));
    let is_local_fs = matches!(base_key.suffix.version(), VersionPart::File(_))
        || meta.is_some_and(|entry| has_local_filesystem_resolution(&entry.resolution));
    let is_local_git = !is_local_fs && is_local_git_package(&base_key, meta);
    if (!is_local_fs && !is_local_git)
        || !policy_authorizes_package(
            scan.policy,
            scan.lockfile,
            (parent_key, parent_meta),
            (field, &alias.to_string(), &base_key, meta),
        )
    {
        return;
    }
    record_policy_package_closure(scan, reachable_snapshots, &base_key, is_local_fs);
}

fn record_policy_package_closure(
    scan: &mut ScopedScan<'_>,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    root_key: &PkgNameVerPeer,
    is_local_fs: bool,
) {
    let mut local_queue = Vec::new();
    let mut git_queue = Vec::new();
    if is_local_fs {
        if scan.closure.packages.insert(root_key.clone()) {
            local_queue.push(root_key.clone());
        }
    } else if scan.closure.git_packages.insert(root_key.clone()) {
        git_queue.push(root_key.clone());
    }
    while let Some(local_key) = local_queue.pop() {
        enqueue_matching_base_snapshots(
            scan,
            reachable_snapshots,
            &local_key,
            (&mut local_queue, &mut git_queue),
        );
    }
    expand_policy_git_snapshots(scan, reachable_snapshots, git_queue);
}

fn enqueue_matching_base_snapshots(
    scan: &mut ScopedScan<'_>,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    local_key: &PkgNameVerPeer,
    (local_queue, git_queue): (&mut Vec<PkgNameVerPeer>, &mut Vec<PkgNameVerPeer>),
) {
    let Some(entries) = scan.snapshots_by_base.get(local_key) else {
        return;
    };
    let packages = scan.lockfile.packages.as_ref();
    for (snap_key, snapshot) in entries {
        if reachable_snapshots.contains(*snap_key) {
            enqueue_reachable_local_children(
                packages,
                snapshot,
                reachable_snapshots,
                &mut scan.closure,
                (local_queue, git_queue),
            );
        }
    }
}

fn enqueue_reachable_local_children(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    snapshot: &SnapshotEntry,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    closure: &mut PolicyClosure,
    (local_queue, git_queue): (&mut Vec<PkgNameVerPeer>, &mut Vec<PkgNameVerPeer>),
) {
    let all_deps = snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    for (alias, dep_ref) in all_deps {
        enqueue_reachable_local_dep_ref(
            packages,
            (alias, dep_ref),
            reachable_snapshots,
            closure,
            (local_queue, git_queue),
        );
    }
}

fn enqueue_reachable_local_dep_ref(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    (alias, dep_ref): (&PkgName, &SnapshotDepRef),
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    closure: &mut PolicyClosure,
    (local_queue, git_queue): (&mut Vec<PkgNameVerPeer>, &mut Vec<PkgNameVerPeer>),
) {
    if let SnapshotDepRef::Link(target) = dep_ref {
        closure.links_by_alias.insert((alias.to_string(), normalize_rel_dir(target)));
        return;
    }
    let Some(child_key) = dep_ref
        .resolve(alias)
        .filter(|key| reachable_snapshots.contains(key))
    else {
        return;
    };
    let base_key = child_key.without_peer();
    let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
    if is_snapshot_backed_by_local_resolution(&base_key, meta) {
        if closure.packages.insert(base_key.clone()) {
            local_queue.push(base_key);
        }
    } else if is_local_git_package(&base_key, meta) && closure.git_packages.insert(base_key.clone())
    {
        git_queue.push(base_key);
    }
}

fn expand_policy_git_snapshots(
    scan: &mut ScopedScan<'_>,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    mut git_queue: Vec<PkgNameVerPeer>,
) {
    let packages = scan.lockfile.packages.as_ref();
    while let Some(git_key) = git_queue.pop() {
        let Some(entries) = scan.snapshots_by_base.get(&git_key) else {
            continue;
        };
        for (snap_key, snapshot) in entries {
            if reachable_snapshots.contains(*snap_key) {
                enqueue_reachable_git_children(
                    packages,
                    snapshot,
                    reachable_snapshots,
                    &mut scan.closure.git_packages,
                    &mut git_queue,
                );
            }
        }
    }
}

fn enqueue_reachable_git_children(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    snapshot: &SnapshotEntry,
    reachable_snapshots: &HashSet<PkgNameVerPeer>,
    policy_git_packages: &mut HashSet<PkgNameVerPeer>,
    git_queue: &mut Vec<PkgNameVerPeer>,
) {
    let all_deps = snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    for (alias, dep_ref) in all_deps {
        let Some(child_key) = dep_ref
            .resolve(alias)
            .filter(|key| reachable_snapshots.contains(key))
        else {
            continue;
        };
        let base_key = child_key.without_peer();
        let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
        if is_local_git_package(&base_key, meta) && policy_git_packages.insert(base_key.clone()) {
            git_queue.push(base_key);
        }
    }
}

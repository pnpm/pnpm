use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageKey,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    ResolvedDependencyMap,
    SnapshotDepRef,
    VersionPart,
};

/// Which lockfile edges a reachability walk follows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LockfileWalk {
    /// Only edges to `file:` packages, from local direct dependencies.
    LocalOnly,
    /// Every edge.
    All,
}

pub(super) fn importer_reachable_lockfile_packages(
    lockfile: &Lockfile,
    importer_id: &str,
    current_direct_aliases: Option<&HashSet<&str>>,
    walk: LockfileWalk,
) -> HashSet<PkgNameVerPeer> {
    let mut visited =
        seed_single_importer_roots(lockfile, importer_id, current_direct_aliases, walk);
    if visited.is_empty() {
        return HashSet::new();
    }
    if let Some(snapshots) = lockfile.snapshots.as_ref() {
        let mut queue: Vec<PackageKey> = visited.iter().cloned().collect();
        while let Some(current) = queue.pop() {
            if let Some(snapshot) = snapshots.get(&current) {
                enqueue_snapshot_dep_map(
                    snapshot.dependencies.as_ref(),
                    walk,
                    &mut visited,
                    &mut queue,
                );
                enqueue_snapshot_dep_map(
                    snapshot.optional_dependencies.as_ref(),
                    walk,
                    &mut visited,
                    &mut queue,
                );
            }
        }
    }
    visited
        .into_iter()
        .map(|key| key.without_peer())
        .collect()
}

fn seed_single_importer_roots(
    lockfile: &Lockfile,
    importer_id: &str,
    current_direct_aliases: Option<&HashSet<&str>>,
    walk: LockfileWalk,
) -> HashSet<PackageKey> {
    let mut roots = HashSet::new();
    if let Some(importer) = lockfile.importers.get(importer_id) {
        for deps in
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        {
            collect_importer_dep_roots(deps.as_ref(), current_direct_aliases, walk, &mut roots);
        }
    }
    if let Some(packages) = lockfile.packages.as_ref() {
        collect_single_importer_dir_pkg(importer_id, packages, &mut roots);
    }
    roots
}

fn collect_importer_dep_roots(
    deps: Option<&ResolvedDependencyMap>,
    current_direct_aliases: Option<&HashSet<&str>>,
    walk: LockfileWalk,
    roots: &mut HashSet<PackageKey>,
) {
    let Some(deps) = deps else {
        return;
    };
    for (alias, dep_spec) in deps {
        let alias_str = alias.to_string();
        if current_direct_aliases.is_some_and(|allowed| !allowed.contains(alias_str.as_str())) {
            continue;
        }
        if let Some(key) = dep_spec.version.resolved_key(alias)
            && (walk == LockfileWalk::All || matches!(key.suffix.version(), VersionPart::File(_)))
        {
            roots.insert(key);
        }
    }
}

fn collect_single_importer_dir_pkg(
    importer_id: &str,
    packages: &HashMap<PackageKey, PackageMetadata>,
    roots: &mut HashSet<PackageKey>,
) {
    for (pkg_key, meta) in packages {
        if matches!(pkg_key.suffix.version(), VersionPart::File(_))
            && matches!(
                &meta.resolution,
                LockfileResolution::Directory(dir) if dir.directory == importer_id,
            )
        {
            roots.insert(pkg_key.clone());
        }
    }
}

fn enqueue_snapshot_dep_map(
    deps: Option<&HashMap<PkgName, SnapshotDepRef>>,
    walk: LockfileWalk,
    visited: &mut HashSet<PackageKey>,
    queue: &mut Vec<PackageKey>,
) {
    let Some(deps) = deps else {
        return;
    };
    for (alias, dep_ref) in deps {
        let Some(child_key) = dep_ref.resolve(alias) else {
            continue;
        };
        if walk == LockfileWalk::LocalOnly
            && !matches!(child_key.suffix.version(), VersionPart::File(_))
        {
            continue;
        }
        if visited.insert(child_key.clone()) {
            queue.push(child_key);
        }
    }
}

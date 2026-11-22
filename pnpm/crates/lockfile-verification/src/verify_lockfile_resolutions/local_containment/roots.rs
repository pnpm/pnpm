use std::collections::{
    HashMap,
    HashSet,
};

use importer_seed::{
    ImporterSeedCtx,
    finish_importer_scope,
    seed_importer,
};
use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    ProjectSnapshot,
    SnapshotDepRef,
    SnapshotEntry,
    VersionPart,
    is_drive_letter_prefix,
    is_tarball_filename,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;
pub(super) use reach::Reached;
use reach::{
    SnapshotGraph,
    Stamps,
};

use super::{
    super::{
        local_git_shape::is_local_git_package,
        offline_checks::is_purely_local_filesystem_resolution,
    },
    LocalResolutionPolicy,
    policy::target_escapes_workspace,
};

mod importer_seed;
mod reach;

pub(super) struct ImporterScope {
    /// The [relevant](SnapshotGraph) snapshots the importer reaches.
    pub(super) relevant_snapshots: HashSet<PkgNameVerPeer>,
    /// The packages of [`Self::relevant_snapshots`].
    pub(super) relevant_packages: HashSet<PkgNameVerPeer>,
    pub(super) local_packages: HashSet<PkgNameVerPeer>,
    pub(super) local_git_packages: HashSet<PkgNameVerPeer>,
    pub(super) links_by_alias: HashSet<(String, String)>,
    pub(super) violations: Vec<ResolutionPolicyViolation>,
}

pub(super) struct AuthorizedLocalRoots<'a> {
    pub(super) workspace_dirs: HashSet<String>,
    pub(super) importers: Vec<ImporterScope>,
    /// Everything the active importers reach together.
    pub(super) reached: Reached,
    pub(super) snapshots_by_base:
        HashMap<PkgNameVerPeer, Vec<(&'a PkgNameVerPeer, &'a SnapshotEntry)>>,
}

impl<'a> AuthorizedLocalRoots<'a> {
    pub(super) fn collect(lockfile: &'a Lockfile, policy: &LocalResolutionPolicy) -> Self {
        let workspace_dirs = collect_workspace_importer_dirs(lockfile, policy);
        let lockfile_importer_dirs = collect_lockfile_importer_dirs(lockfile);
        let snapshots_by_base = index_local_snapshots(lockfile);
        let seeds: Vec<_> = active_importers(lockfile, policy, &workspace_dirs)
            .into_iter()
            .map(|(imp_id, imp_snap)| {
                let seed_ctx = ImporterSeedCtx {
                    lockfile,
                    policy,
                    workspace_dirs: &workspace_dirs,
                    lockfile_importer_dirs: &lockfile_importer_dirs,
                    importer_id: &imp_id,
                };
                seed_importer(&seed_ctx, imp_snap)
            })
            .collect();
        let all_seeds = || seeds.iter().flat_map(|bufs| &bufs.all_seed);
        let graph = SnapshotGraph::build(lockfile, all_seeds());
        let reached = graph.reach_all(all_seeds());
        let mut stamps = Stamps::default();
        let importers = seeds
            .into_iter()
            .map(|bufs| {
                let relevant = graph.reach_relevant(&bufs.all_seed, &mut stamps);
                finish_importer_scope(lockfile, bufs, relevant)
            })
            .collect();
        Self { workspace_dirs, importers, reached, snapshots_by_base }
    }
}

pub(super) fn is_snapshot_backed_by_local_resolution(
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    let VersionPart::File(key_path) = base_key.suffix.version() else {
        return false;
    };
    let Some(meta) = meta else {
        return true;
    };
    is_purely_local_filesystem_resolution(&meta.resolution)
        || (is_tarball_filename(key_path)
            && matches!(meta.resolution, LockfileResolution::Registry(_)))
}

pub(in super::super) fn is_non_relative_target(target: &str) -> bool {
    target.starts_with('/')
        || target.contains('\\')
        || target == "~"
        || target.starts_with("~/")
        || is_drive_letter_prefix(target)
}

pub(in super::super) fn normalize_rel_dir(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments
                    .last()
                    .is_some_and(|last| *last != "..")
                {
                    segments.pop();
                } else if !rooted {
                    segments.push("..");
                }
            }
            _ => segments.push(segment),
        }
    }
    if rooted {
        format!("/{}", segments.join("/"))
    } else if segments.is_empty() {
        ".".to_string()
    } else {
        segments.join("/")
    }
}

/// Whether `path` steps up (`..`) after descending into a named segment, as
/// in `linked/../shared`. Lexical normalization collapses such a path, but
/// the filesystem resolves it through `linked`, which may be a symlink.
pub(in super::super) fn has_inner_parent_segment(path: &str) -> bool {
    let mut descended = false;
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." if descended => return true,
            ".." => {}
            _ => descended = true,
        }
    }
    false
}

pub(super) fn anchor_importer_link(importer_id: &str, target: &str) -> String {
    if importer_id == "." || is_non_relative_target(target) {
        normalize_rel_dir(target)
    } else {
        normalize_rel_dir(&format!("{importer_id}/{target}"))
    }
}

fn active_importers<'a>(
    lockfile: &'a Lockfile,
    policy: &LocalResolutionPolicy,
    workspace_dirs: &HashSet<String>,
) -> Vec<(String, &'a ProjectSnapshot)> {
    let selected: Option<HashSet<String>> = policy.scope.selected_importer_ids
        .as_deref()
        .map(|ids| {
            ids.iter()
                .map(|id| normalize_rel_dir(id))
                .collect()
        });
    let restrict_live = policy.scope.live_workspace_dirs.is_some();
    let mut out = Vec::new();
    for (importer_id, importer) in &lockfile.importers {
        let norm_id = normalize_rel_dir(importer_id);
        if let Some(sel) = &selected {
            if sel.contains(&norm_id) {
                out.push((norm_id, importer));
            }
        } else if !restrict_live || workspace_dirs.contains(&norm_id) {
            out.push((norm_id, importer));
        }
    }
    out
}

pub(super) fn expand_local_snapshot_closure(
    lockfile: &Lockfile,
    mut local_queue: Vec<PkgNameVerPeer>,
    mut git_queue: Vec<PkgNameVerPeer>,
    (local_packages, local_git_packages, links_by_alias): (
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<(String, String)>,
    ),
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return;
    };
    let packages = lockfile.packages.as_ref();
    let mut visited_local: HashSet<PkgNameVerPeer> = local_queue.iter().cloned().collect();
    let mut visited_git: HashSet<PkgNameVerPeer> = git_queue.iter().cloned().collect();
    while let Some(snap_key) = local_queue.pop() {
        let Some(snapshot) = snapshots.get(&snap_key) else {
            continue;
        };
        let all_deps = snapshot.dependencies
            .iter()
            .flatten()
            .chain(snapshot.optional_dependencies.iter().flatten());
        enqueue_local_dep_refs(
            packages,
            all_deps,
            (local_packages, local_git_packages, links_by_alias),
            (&mut visited_local, &mut local_queue, &mut visited_git, &mut git_queue),
        );
    }
    expand_local_git_closure(snapshots, packages, &mut visited_git, git_queue, local_git_packages);
}

fn expand_local_git_closure(
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    visited_git: &mut HashSet<PkgNameVerPeer>,
    mut git_queue: Vec<PkgNameVerPeer>,
    local_git_packages: &mut HashSet<PkgNameVerPeer>,
) {
    while let Some(snap_key) = git_queue.pop() {
        if let Some(snapshot) = snapshots.get(&snap_key) {
            enqueue_local_git_children(
                packages,
                snapshot,
                visited_git,
                &mut git_queue,
                local_git_packages,
            );
        }
    }
}

fn enqueue_local_git_children(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    snapshot: &SnapshotEntry,
    visited_git: &mut HashSet<PkgNameVerPeer>,
    git_queue: &mut Vec<PkgNameVerPeer>,
    local_git_packages: &mut HashSet<PkgNameVerPeer>,
) {
    let all_deps = snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    for (alias, dep_ref) in all_deps {
        let Some(child_key) = dep_ref.resolve(alias) else {
            continue;
        };
        let base_key = child_key.without_peer();
        let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
        if is_local_git_package(&base_key, meta) {
            local_git_packages.insert(base_key);
            if visited_git.insert(child_key.clone()) {
                git_queue.push(child_key);
            }
        }
    }
}

fn enqueue_local_dep_refs<'a>(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    deps: impl IntoIterator<Item = (&'a PkgName, &'a SnapshotDepRef)>,
    (local_packages, local_git_packages, links_by_alias): (
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<(String, String)>,
    ),
    (visited_local, local_queue, visited_git, git_queue): (
        &mut HashSet<PkgNameVerPeer>,
        &mut Vec<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
        &mut Vec<PkgNameVerPeer>,
    ),
) {
    for (alias, dep_ref) in deps {
        if let SnapshotDepRef::Link(target) = dep_ref {
            links_by_alias.insert((alias.to_string(), normalize_rel_dir(target)));
            continue;
        }
        if let Some(child_key) = dep_ref.resolve(alias) {
            enqueue_resolved_local_child(
                packages,
                child_key,
                (local_packages, local_git_packages),
                (visited_local, local_queue, visited_git, git_queue),
            );
        }
    }
}

fn enqueue_resolved_local_child(
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    child_key: PkgNameVerPeer,
    (local_packages, local_git_packages): (
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
    ),
    (visited_local, local_queue, visited_git, git_queue): (
        &mut HashSet<PkgNameVerPeer>,
        &mut Vec<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
        &mut Vec<PkgNameVerPeer>,
    ),
) {
    let base_key = child_key.without_peer();
    let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
    if is_snapshot_backed_by_local_resolution(&base_key, meta) {
        local_packages.insert(base_key);
        if visited_local.insert(child_key.clone()) {
            local_queue.push(child_key);
        }
    } else if is_local_git_package(&base_key, meta) {
        local_git_packages.insert(base_key);
        if visited_git.insert(child_key.clone()) {
            git_queue.push(child_key);
        }
    }
}

fn index_local_snapshots(
    lockfile: &Lockfile,
) -> HashMap<PkgNameVerPeer, Vec<(&PkgNameVerPeer, &SnapshotEntry)>> {
    let mut by_base: HashMap<PkgNameVerPeer, Vec<(&PkgNameVerPeer, &SnapshotEntry)>> =
        HashMap::new();
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return by_base;
    };
    let packages = lockfile.packages.as_ref();
    for (snap_key, snapshot) in snapshots {
        let base_key = snap_key.without_peer();
        let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
        if is_snapshot_backed_by_local_resolution(&base_key, meta)
            || is_local_git_package(&base_key, meta)
        {
            by_base
                .entry(base_key)
                .or_default()
                .push((snap_key, snapshot));
        }
    }
    by_base
}

/// Relative directories of every importer recorded in `lockfile`, live or not.
fn collect_lockfile_importer_dirs(lockfile: &Lockfile) -> HashSet<String> {
    lockfile.importers
        .keys()
        .filter(|importer_id| !is_non_relative_target(importer_id))
        .map(|importer_id| normalize_rel_dir(importer_id))
        .collect()
}

fn collect_workspace_importer_dirs(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
) -> HashSet<String> {
    if let Some(live_dirs) = policy.scope.live_workspace_dirs.as_deref() {
        return live_dirs
            .iter()
            .map(|dir| normalize_rel_dir(dir))
            .collect();
    }
    let mut dirs = HashSet::new();
    for (importer_id, importer) in &lockfile.importers {
        add_contained_importer_dirs(
            policy,
            importer_id,
            importer.publish_directory.as_deref(),
            &mut dirs,
        );
    }
    dirs
}

fn add_contained_importer_dirs(
    policy: &LocalResolutionPolicy,
    importer_id: &str,
    publish_directory: Option<&str>,
    dirs: &mut HashSet<String>,
) {
    if is_non_relative_target(importer_id) || target_escapes_workspace(policy, importer_id) {
        return;
    }
    let normalized_id = normalize_rel_dir(importer_id);
    dirs.insert(normalized_id.clone());
    let Some(pub_dir) = publish_directory else {
        return;
    };
    let combined = anchor_importer_link(&normalized_id, pub_dir);
    if !is_non_relative_target(&combined) && !target_escapes_workspace(policy, &combined) {
        dirs.insert(combined);
    }
}

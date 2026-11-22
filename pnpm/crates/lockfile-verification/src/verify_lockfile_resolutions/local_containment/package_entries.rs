use std::{
    cell::OnceCell,
    collections::{
        HashMap,
        HashSet,
    },
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageMetadata,
    PeerBindings,
    PkgNameVerPeer,
    VersionPart,
    is_non_local_filesystem_subdep_spec,
    package_root_link_target,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;

use super::{
    super::{
        LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
        RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
        candidates::file_key_mismatches_local_target,
        file_url_shape::FileShapeContext,
        local_git_shape::{
            extract_local_git_target,
            is_local_git_package,
            local_git_key_mismatches_target,
            local_git_key_raw_path,
        },
        offline_checks::strip_file_url_prefix,
    },
    LocalResolutionPolicy,
    is_deploy_entry,
    is_workspace_directory_entry,
    policy::{
        policy_authorizes_peer_spec,
        target_escapes_workspace,
    },
    roots::{
        ImporterScope,
        is_snapshot_backed_by_local_resolution,
    },
};

pub(super) struct PackageEntryScan<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) policy: &'a LocalResolutionPolicy,
    pub(super) workspace_dirs: &'a HashSet<String>,
    pub(super) authorized_packages: &'a HashSet<PkgNameVerPeer>,
    pub(super) authorized_git_packages: &'a HashSet<PkgNameVerPeer>,
    /// Set when an importer selection narrows the scan.
    pub(super) selection: Option<ImporterSelection<'a>>,
    pub(super) importers: &'a [ImporterScope],
    pub(super) peer_indexes: PeerIndexes<'a>,
}

/// What the selected importers reach.
#[derive(Clone, Copy)]
pub(super) struct ImporterSelection<'a> {
    pub(super) packages: &'a HashSet<PkgNameVerPeer>,
    /// Peers resolved only in other snapshots do not count.
    pub(super) snapshots: &'a HashSet<&'a PkgNameVerPeer>,
}

/// A snapshot key and the aliases its peer suffix binds.
type SnapshotPeers<'a> = (&'a PkgNameVerPeer, HashSet<String>);

/// Peer indexes a scan builds on first use.
#[derive(Default)]
pub(super) struct PeerIndexes<'a> {
    /// Built by [`has_unvouched_resolving_snapshot`].
    resolved_aliases: OnceCell<HashMap<PkgNameVerPeer, Vec<SnapshotPeers<'a>>>>,
    /// Built by [`importer_vouches_for_peer`].
    declarations: OnceCell<Vec<HashSet<(&'a str, &'a str)>>>,
}

pub(super) fn collect_package_entry_violations(
    scan: &PackageEntryScan<'_>,
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let Some(packages) = scan.lockfile.packages.as_ref() else {
        return;
    };
    let shape_ctx = FileShapeContext {
        lockfile_dir: scan.policy.scope.lockfile_dir.as_deref(),
        allow_deploy_file_url_keys: scan.policy.allow_deploy_file_url_keys,
    };
    for (pkg_key, meta) in packages {
        if scan.selection.is_some_and(|selection| !selection.packages.contains(pkg_key)) {
            continue;
        }
        check_package_peer_specs(scan, pkg_key, meta, violations);
        check_file_or_local_git_entry(scan, shape_ctx, pkg_key, meta, violations);
    }
}

fn check_file_or_local_git_entry(
    scan: &PackageEntryScan<'_>,
    shape_ctx: FileShapeContext<'_>,
    pkg_key: &PkgNameVerPeer,
    meta: &PackageMetadata,
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    if let VersionPart::File(key_path) = pkg_key.suffix.version() {
        if file_key_mismatches_local_target(key_path, &meta.resolution, shape_ctx) {
            violations.push(package_violation(
                pkg_key,
                Some(meta),
                RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
                "a file dependency path does not match its local resolution target",
            ));
        } else if !is_authorized_pkg_entry(scan, pkg_key, meta)
            && resolution_escapes_workspace(&meta.resolution, scan.policy)
        {
            violations.push(package_violation(
                pkg_key,
                Some(meta),
                LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
                "a local filesystem package entry escapes outside the workspace without authorization",
            ));
        }
        return;
    }
    if let Some(git_key_path) = local_git_key_raw_path(pkg_key) {
        if local_git_key_mismatches_target(git_key_path, &meta.resolution, shape_ctx) {
            violations.push(package_violation(
                pkg_key,
                Some(meta),
                RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
                "a local git dependency path does not match its git repository target",
            ));
        } else if !scan.authorized_git_packages.contains(pkg_key)
            && resolution_escapes_workspace(&meta.resolution, scan.policy)
        {
            violations.push(package_violation(
                pkg_key,
                Some(meta),
                LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
                "a local git package entry escapes outside the workspace without authorization",
            ));
        }
    }
}

fn is_authorized_pkg_entry(
    scan: &PackageEntryScan<'_>,
    pkg_key: &PkgNameVerPeer,
    meta: &PackageMetadata,
) -> bool {
    is_deploy_entry(scan.policy, pkg_key, Some(meta))
        || scan.authorized_packages.contains(pkg_key)
        || is_workspace_directory_entry(scan.policy, scan.workspace_dirs, pkg_key, Some(meta))
}

pub(super) fn package_violation(
    key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
    code: &'static str,
    reason: &'static str,
) -> ResolutionPolicyViolation {
    let resolution = meta
        .map(|entry| entry.resolution.clone())
        .unwrap_or_else(|| {
            LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
                directory: key.suffix.version().to_string(),
            })
        });
    ResolutionPolicyViolation {
        name: key.name.clone(),
        version: key.suffix.version().to_string(),
        resolution,
        code,
        reason: reason.to_string(),
    }
}

fn check_package_peer_specs(
    scan: &PackageEntryScan<'_>,
    pkg_key: &PkgNameVerPeer,
    meta: &PackageMetadata,
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let Some(peers) = meta.peer_dependencies.as_ref() else {
        return;
    };
    if is_snapshot_backed_by_local_resolution(pkg_key, Some(meta))
        && is_authorized_pkg_entry(scan, pkg_key, meta)
    {
        return;
    }
    let is_authorized_git = is_local_git_package(pkg_key, Some(meta))
        && scan.authorized_git_packages.contains(pkg_key);
    for (alias, spec) in peers {
        if is_unauthorized_peer_spec(scan, (pkg_key, meta), is_authorized_git, (alias, spec))
            && has_unvouched_resolving_snapshot(scan, pkg_key, (alias, spec))
        {
            violations.push(package_violation(
                pkg_key,
                Some(meta),
                LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
                "a package entry declares an unauthorized local filesystem peer dependency",
            ));
        }
    }
}

fn is_unauthorized_peer_spec(
    scan: &PackageEntryScan<'_>,
    (pkg_key, meta): (&PkgNameVerPeer, &PackageMetadata),
    is_authorized_git: bool,
    (alias, spec): (&String, &String),
) -> bool {
    if is_authorized_git && spec_is_git_file(spec) {
        return false;
    }
    let is_internal = spec
        .strip_prefix("link:")
        .and_then(package_root_link_target)
        .is_some();
    !is_internal
        && is_non_local_filesystem_subdep_spec(spec, |_| false)
        && !policy_authorizes_peer_spec(scan.policy, scan.lockfile, (pkg_key, meta), alias, spec)
}

/// Whether a snapshot of `pkg_key` resolves the peer `alias` while no importer
/// that reaches it vouches for `spec`. A peer left unresolved (for example
/// under `autoInstallPeers: false`) creates no edge, so its declaration
/// reaches nothing on disk.
fn has_unvouched_resolving_snapshot(
    scan: &PackageEntryScan<'_>,
    pkg_key: &PkgNameVerPeer,
    (alias, spec): (&str, &str),
) -> bool {
    let selected_snapshots = scan.selection.map(|selection| selection.snapshots);
    scan.peer_indexes.resolved_aliases
        .get_or_init(|| index_resolved_aliases(scan.lockfile, selected_snapshots))
        .get(pkg_key)
        .into_iter()
        .flatten()
        .filter(|(_, aliases)| aliases.contains(alias))
        .any(|(snap_key, _)| !importer_vouches_for_peer(scan, snap_key, (alias, spec)))
}

fn index_resolved_aliases<'a>(
    lockfile: &'a Lockfile,
    selected_snapshots: Option<&HashSet<&PkgNameVerPeer>>,
) -> HashMap<PkgNameVerPeer, Vec<SnapshotPeers<'a>>> {
    let mut by_base: HashMap<PkgNameVerPeer, Vec<SnapshotPeers<'a>>> = HashMap::new();
    let snapshots = lockfile.snapshots
        .iter()
        .flatten()
        .filter(|(snap_key, _)| {
            selected_snapshots.is_none_or(|selected| selected.contains(snap_key))
        });
    for (snap_key, snapshot) in snapshots {
        let peer_suffix = snap_key.suffix.peer();
        let names = [&snapshot.dependencies, &snapshot.optional_dependencies]
            .into_iter()
            .flatten()
            .flat_map(HashMap::keys)
            .map(ToString::to_string)
            .filter(|name| peer_suffix_binds(peer_suffix, name))
            .collect();
        by_base
            .entry(snap_key.without_peer())
            .or_default()
            .push((snap_key, names));
    }
    by_base
}

/// Whether the snapshot key's peer suffix binds `alias`. A regular
/// dependency of the same name is not a resolved peer. A hashed suffix names
/// no peers, so it counts as binding every alias.
fn peer_suffix_binds(peer_suffix: &str, alias: &str) -> bool {
    match PeerBindings::from_suffix(peer_suffix) {
        None => false,
        Some(PeerBindings::Hashed) => true,
        Some(PeerBindings::Explicit(segments)) => segments
            .iter()
            .any(|segment| {
                segment
                    .strip_prefix(alias)
                    .is_some_and(|rest| rest.starts_with('@'))
            }),
    }
}

/// Whether an importer that reaches `snap_key` also reaches an authorized
/// local package declaring the same peer. Resolution hoists that local peer
/// for the importer's non-local packages too, never for another importer's.
fn importer_vouches_for_peer(
    scan: &PackageEntryScan<'_>,
    snap_key: &PkgNameVerPeer,
    (alias, spec): (&str, &str),
) -> bool {
    scan.peer_indexes.declarations
        .get_or_init(|| index_authorized_peer_declarations(scan))
        .iter()
        .zip(scan.importers)
        .any(|(declared, importer)| {
            importer.relevant_snapshots.contains(snap_key) && declared.contains(&(alias, spec))
        })
}

/// The `(alias, spec)` peers that the authorized local packages each importer
/// reaches declare, in [`PackageEntryScan::importers`] order.
fn index_authorized_peer_declarations<'a>(
    scan: &PackageEntryScan<'a>,
) -> Vec<HashSet<(&'a str, &'a str)>> {
    let packages = scan.lockfile.packages.as_ref();
    scan.importers
        .iter()
        .map(|importer| {
            importer.relevant_packages
                .iter()
                .filter(|key| scan.authorized_packages.contains(*key))
                .filter_map(|key| packages?.get(key)?.peer_dependencies.as_ref())
                .flatten()
                .map(|(alias, spec)| (alias.as_str(), spec.as_str()))
                .collect()
        })
        .collect()
}

fn spec_is_git_file(spec: &str) -> bool {
    spec.trim()
        .get(.."git+file:".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("git+file:"))
}

fn resolution_escapes_workspace(
    resolution: &LockfileResolution,
    policy: &LocalResolutionPolicy,
) -> bool {
    match resolution {
        LockfileResolution::Directory(dir) => target_escapes_workspace(policy, &dir.directory),
        LockfileResolution::Tarball(tarball) => strip_file_url_prefix(&tarball.tarball)
            .is_some_and(|tar_path| target_escapes_workspace(policy, tar_path)),
        LockfileResolution::Git(git) => extract_local_git_target(&git.repo)
            .is_some_and(|git_path| target_escapes_workspace(policy, &git_path)),
        LockfileResolution::Variations(variations) => variations.variants
            .iter()
            .any(|variant| resolution_escapes_workspace(&variant.resolution, policy)),
        _ => false,
    }
}

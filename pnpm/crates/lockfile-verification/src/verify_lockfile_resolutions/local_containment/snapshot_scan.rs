use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    Lockfile,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    SnapshotDepRef,
    SnapshotEntry,
    VersionPart,
    package_root_link_target,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;

use super::{
    super::{
        LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
        local_git_shape::is_local_git_package,
        offline_checks::has_local_filesystem_resolution,
    },
    LocalDepField,
    LocalResolutionPolicy,
    is_allowed_workspace_target,
    is_deploy_entry,
    is_workspace_directory_entry,
    link_violation_with_code,
    package_entries::package_violation,
    policy::{
        policy_authorizes_link,
        policy_authorizes_package,
    },
    policy_seeding::seed_reachable_policy_closures,
    roots::{
        AuthorizedLocalRoots,
        ImporterScope,
        is_snapshot_backed_by_local_resolution,
        normalize_rel_dir,
    },
};

#[derive(Default)]
pub(super) struct PolicyClosure {
    pub(super) packages: HashSet<PkgNameVerPeer>,
    pub(super) git_packages: HashSet<PkgNameVerPeer>,
    pub(super) links_by_alias: HashSet<(String, String)>,
}

pub(super) struct ScopedScan<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) policy: &'a LocalResolutionPolicy,
    pub(super) workspace_dirs: &'a HashSet<String>,
    pub(super) snapshots_by_base:
        &'a HashMap<PkgNameVerPeer, Vec<(&'a PkgNameVerPeer, &'a SnapshotEntry)>>,
    pub(super) local_packages: &'a HashSet<PkgNameVerPeer>,
    pub(super) local_git_packages: &'a HashSet<PkgNameVerPeer>,
    pub(super) closure: PolicyClosure,
}

pub(super) fn scan_importer_scope(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
    roots: &AuthorizedLocalRoots<'_>,
    imp: &ImporterScope,
    (authorized_packages, authorized_git_packages, violations): (
        &mut HashSet<PkgNameVerPeer>,
        &mut HashSet<PkgNameVerPeer>,
        &mut Vec<ResolutionPolicyViolation>,
    ),
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return;
    };
    let mut scan = ScopedScan {
        lockfile,
        policy,
        workspace_dirs: &roots.workspace_dirs,
        snapshots_by_base: &roots.snapshots_by_base,
        local_packages: &imp.local_packages,
        local_git_packages: &imp.local_git_packages,
        closure: PolicyClosure {
            links_by_alias: imp.links_by_alias.clone(),
            ..PolicyClosure::default()
        },
    };
    if policy.has_authorization_source() {
        seed_reachable_policy_closures(&mut scan, snapshots, &imp.relevant_snapshots);
        authorized_packages.extend(scan.closure.packages.iter().cloned());
        authorized_git_packages.extend(scan.closure.git_packages.iter().cloned());
    }
    for snap_key in &imp.relevant_snapshots {
        if let Some(snapshot) = snapshots.get(snap_key) {
            inspect_snapshot_entry(&scan, snap_key, snapshot, violations);
        }
    }
}

pub(super) fn scan_unreachable_snapshots(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
    roots: &AuthorizedLocalRoots<'_>,
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return;
    };
    let empty_packages = HashSet::new();
    let scan = ScopedScan {
        lockfile,
        policy,
        workspace_dirs: &roots.workspace_dirs,
        snapshots_by_base: &roots.snapshots_by_base,
        local_packages: &empty_packages,
        local_git_packages: &empty_packages,
        closure: PolicyClosure::default(),
    };
    for (snap_key, snapshot) in snapshots {
        if !roots.reached.snapshots.contains(snap_key) {
            inspect_snapshot_entry(&scan, snap_key, snapshot, violations);
        }
    }
}

fn inspect_snapshot_entry(
    scan: &ScopedScan<'_>,
    snap_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let base_snap = snap_key.without_peer();
    let snap_meta = scan.lockfile.packages
        .as_ref()
        .and_then(|pkgs| pkgs.get(&base_snap));
    if is_snapshot_backed_by_local_resolution(&base_snap, snap_meta)
        && is_authorized_local_package(scan, &base_snap, snap_meta)
    {
        return;
    }
    let parent_is_authorized_git = is_local_git_package(&base_snap, snap_meta)
        && (scan.local_git_packages.contains(&base_snap)
            || scan.closure.git_packages.contains(&base_snap));
    for (field, deps) in [
        (LocalDepField::Dependencies, &snapshot.dependencies),
        (LocalDepField::OptionalDependencies, &snapshot.optional_dependencies),
    ] {
        for (alias, dep_ref) in deps.iter().flatten() {
            inspect_non_local_child(
                scan,
                (snap_key, snap_meta, parent_is_authorized_git),
                (field, alias, dep_ref),
                violations,
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LocalChildKind {
    NonLocal,
    LocalFs,
    LocalGit,
}

fn classify_local_child(
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> LocalChildKind {
    if matches!(base_key.suffix.version(), VersionPart::File(_))
        || meta.is_some_and(|entry| has_local_filesystem_resolution(&entry.resolution))
    {
        LocalChildKind::LocalFs
    } else if is_local_git_package(base_key, meta) {
        LocalChildKind::LocalGit
    } else {
        LocalChildKind::NonLocal
    }
}

fn inspect_non_local_child(
    scan: &ScopedScan<'_>,
    (parent_key, parent_meta, parent_is_authorized_git): (
        &PkgNameVerPeer,
        Option<&PackageMetadata>,
        bool,
    ),
    (field, alias, dep_ref): (LocalDepField, &PkgName, &SnapshotDepRef),
    violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let alias_str = alias.to_string();
    if let SnapshotDepRef::Link(target) = dep_ref {
        if !is_allowed_snapshot_link(scan, (parent_key, parent_meta), field, &alias_str, target) {
            violations.push(link_violation_with_code(
                parent_key.name.clone(),
                parent_key.suffix.version().to_string(),
                target.clone(),
                LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
                "a non-local package snapshot links to an unauthorized host filesystem path",
            ));
        }
        return;
    }
    let Some(child_key) = dep_ref.resolve(alias) else {
        return;
    };
    let base_key = child_key.without_peer();
    let meta = scan.lockfile.packages
        .as_ref()
        .and_then(|pkgs| pkgs.get(&base_key));
    let kind = classify_local_child(&base_key, meta);
    if kind == LocalChildKind::NonLocal
        || (kind == LocalChildKind::LocalGit && parent_is_authorized_git)
    {
        return;
    }
    let is_local_git = kind == LocalChildKind::LocalGit;
    if !allow_non_local_child_edge(
        scan,
        (parent_key, parent_meta),
        (field, &alias_str, &base_key, meta, is_local_git),
    ) {
        violations.push(package_violation(
            parent_key,
            meta,
            LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
            "a non-local package snapshot references an unauthorized local filesystem dependency",
        ));
    }
}

fn is_allowed_snapshot_link(
    scan: &ScopedScan<'_>,
    (parent_key, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    field: LocalDepField,
    alias: &str,
    target: &str,
) -> bool {
    if package_root_link_target(target).is_some() {
        return true;
    }
    if target.starts_with("<root>") {
        return false;
    }
    if policy_authorizes_link(
        scan.policy,
        scan.lockfile,
        (parent_key, parent_meta),
        (field, alias, target),
    ) {
        return true;
    }
    let key = (alias.to_string(), normalize_rel_dir(target));
    let peer_allowed =
        parent_declares_peer(parent_meta, alias) && scan.closure.links_by_alias.contains(&key);
    peer_allowed || is_allowed_workspace_target(scan.workspace_dirs, target)
}

fn allow_non_local_child_edge(
    scan: &ScopedScan<'_>,
    (parent_key, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    (field, alias, base_key, meta, is_local_git): (
        LocalDepField,
        &str,
        &PkgNameVerPeer,
        Option<&PackageMetadata>,
        bool,
    ),
) -> bool {
    let peer_roots_match = if is_local_git {
        scan.local_git_packages.contains(base_key) || scan.closure.git_packages.contains(base_key)
    } else {
        scan.local_packages.contains(base_key) || scan.closure.packages.contains(base_key)
    };
    is_deploy_entry(scan.policy, base_key, meta)
        || is_workspace_directory_entry(scan.policy, scan.workspace_dirs, base_key, meta)
        || (parent_declares_peer(parent_meta, alias) && peer_roots_match)
        || policy_authorizes_package(
            scan.policy,
            scan.lockfile,
            (parent_key, parent_meta),
            (field, alias, base_key, meta),
        )
}

fn is_authorized_local_package(
    scan: &ScopedScan<'_>,
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    is_deploy_entry(scan.policy, base_key, meta)
        || scan.local_packages.contains(base_key)
        || scan.closure.packages.contains(base_key)
        || is_workspace_directory_entry(scan.policy, scan.workspace_dirs, base_key, meta)
}

fn parent_declares_peer(meta: Option<&PackageMetadata>, alias: &str) -> bool {
    let Some(meta) = meta else {
        return false;
    };
    meta.peer_dependencies
        .as_ref()
        .is_some_and(|peers| peers.contains_key(alias))
        || meta.peer_dependencies_meta
            .as_ref()
            .is_some_and(|peers| peers.contains_key(alias))
}

//! Offline verification that local-filesystem (`directory`, `file:` tarball,
//! `link:`, and `git+file:` / local `git`) entries in a lockfile are rooted in
//! workspace importers or authorized workspace configuration rather than
//! injected by non-local packages.

pub(super) mod roots;

pub use policy::{
    LocalDepField,
    LocalResolutionPolicy,
    PristineDeclaredSpecs,
};
pub use probe::{
    LocalResolutionProbeParent,
    local_resolution_candidate_parents,
    local_resolution_candidate_parents_for_policy,
};

use std::collections::HashSet;

use fast_scan::lockfile_has_any_local_surface;
use package_entries::{
    PackageEntryScan,
    collect_package_entry_violations,
};
use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    VersionPart,
    escapes_workspace_root,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;
use roots::{
    AuthorizedLocalRoots,
    has_inner_parent_segment,
    is_non_relative_target,
    normalize_rel_dir,
};
use snapshot_scan::{
    scan_importer_scope,
    scan_unreachable_snapshots,
};

use super::{
    candidates::{
        build_verification_error,
        file_key_mismatches_local_target,
    },
    file_url_shape::FileShapeContext,
    offline_checks::{
        has_local_filesystem_resolution,
        verify_lockfile_resolution_shapes_with_ctx,
    },
};
use crate::errors::VerifyError;

mod fast_scan;
mod package_entries;
mod policy;
mod policy_seeding;
mod probe;
mod snapshot_scan;

/// Reject a lockfile whose non-importer snapshots reference host-filesystem
/// `directory`, `file:` tarball, `link:`, or local `git` targets without
/// workspace authorization, or whose local entries escape the workspace without
/// an importer or hook declaring them.
pub fn verify_lockfile_local_resolutions(
    lockfile: &Lockfile,
    local_policy: Option<LocalResolutionPolicy>,
) -> Result<(), VerifyError> {
    if !lockfile_has_any_local_surface(lockfile) {
        return Ok(());
    }
    let effective = local_policy.unwrap_or_default();
    let violations = collect_local_resolution_violations(lockfile, &effective);
    let shape_ctx = FileShapeContext {
        lockfile_dir: effective.scope.lockfile_dir.as_deref(),
        allow_deploy_file_url_keys: effective.allow_deploy_file_url_keys,
    };
    verify_lockfile_resolution_shapes_with_ctx(lockfile, shape_ctx)?;
    if violations.is_empty() {
        return Ok(());
    }
    Err(build_verification_error(violations))
}

fn collect_local_resolution_violations(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
) -> Vec<ResolutionPolicyViolation> {
    let roots = AuthorizedLocalRoots::collect(lockfile, policy);
    let mut raw_violations = Vec::new();
    let (reachable_packages, authorized_packages, authorized_git_packages) =
        scan_all_scopes(lockfile, policy, &roots, &mut raw_violations);
    let reachable_filter = policy.scope.selected_importer_ids.as_ref().map(|_| &reachable_packages);
    let pkg_scan = PackageEntryScan {
        lockfile,
        policy,
        workspace_dirs: &roots.workspace_dirs,
        authorized_packages: &authorized_packages,
        authorized_git_packages: &authorized_git_packages,
        reachable_filter,
        resolved_aliases: std::cell::OnceCell::new(),
    };
    collect_package_entry_violations(&pkg_scan, &mut raw_violations);
    dedupe_violations(raw_violations)
}

fn scan_all_scopes(
    lockfile: &Lockfile,
    policy: &LocalResolutionPolicy,
    roots: &AuthorizedLocalRoots<'_>,
    raw_violations: &mut Vec<ResolutionPolicyViolation>,
) -> (HashSet<PkgNameVerPeer>, HashSet<PkgNameVerPeer>, HashSet<PkgNameVerPeer>) {
    let mut visited_snapshots = HashSet::new();
    let mut reachable_packages = HashSet::new();
    let mut authorized_packages = HashSet::new();
    let mut authorized_git_packages = HashSet::new();
    for imp in &roots.importers {
        raw_violations.extend(imp.violations.iter().cloned());
        reachable_packages.extend(imp.reachable_packages.iter().cloned());
        authorized_packages.extend(imp.local_packages.iter().cloned());
        authorized_git_packages.extend(imp.local_git_packages.iter().cloned());
        scan_importer_scope(
            lockfile,
            policy,
            roots,
            imp,
            (
                &mut visited_snapshots,
                &mut authorized_packages,
                &mut authorized_git_packages,
                raw_violations,
            ),
        );
    }
    if policy.scope.selected_importer_ids.is_none() {
        scan_unreachable_snapshots(lockfile, policy, roots, &visited_snapshots, raw_violations);
    }
    (reachable_packages, authorized_packages, authorized_git_packages)
}

pub(super) fn escapes_above_depth(rel_path: &str, workspace_depth: usize) -> bool {
    let anchor =
        if workspace_depth == 0 { ".".to_string() } else { vec!["d"; workspace_depth].join("/") };
    escapes_workspace_root(&anchor, rel_path)
}

pub(super) fn is_allowed_workspace_target(workspace_dirs: &HashSet<String>, target: &str) -> bool {
    !is_non_relative_target(target)
        && !has_inner_parent_segment(target)
        && workspace_dirs.contains(&normalize_rel_dir(target))
}

pub(super) fn is_deploy_entry(
    policy: &LocalResolutionPolicy,
    key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    let VersionPart::File(key_path) = key.suffix.version() else {
        return false;
    };
    let Some(meta) = meta else {
        return false;
    };
    policy.allow_deploy_file_url_keys
        && key_path.starts_with("//")
        && has_local_filesystem_resolution(&meta.resolution)
        && !file_key_mismatches_local_target(
            key_path,
            &meta.resolution,
            FileShapeContext {
                lockfile_dir: policy.scope.lockfile_dir.as_deref(),
                allow_deploy_file_url_keys: true,
            },
        )
}

pub(super) fn is_workspace_directory_entry(
    policy: &LocalResolutionPolicy,
    workspace_dirs: &HashSet<String>,
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    match meta.map(|entry| &entry.resolution) {
        Some(LockfileResolution::Directory(dir)) => {
            is_allowed_workspace_target(workspace_dirs, &dir.directory)
        }
        None if policy.allow_deploy_file_url_keys => match base_key.suffix.version() {
            VersionPart::File(path) => is_allowed_workspace_target(workspace_dirs, path),
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn link_violation_with_code(
    name: PkgName,
    version: String,
    directory: String,
    code: &'static str,
    reason: &'static str,
) -> ResolutionPolicyViolation {
    ResolutionPolicyViolation {
        name,
        version,
        resolution: LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution { directory }),
        code,
        reason: reason.to_string(),
    }
}

fn dedupe_violations(raw: Vec<ResolutionPolicyViolation>) -> Vec<ResolutionPolicyViolation> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(raw.len());
    for violation in raw {
        let key = (
            violation.name.to_string(),
            violation.version.clone(),
            violation.code,
            violation.reason.clone(),
        );
        if seen.insert(key) {
            out.push(violation);
        }
    }
    out
}

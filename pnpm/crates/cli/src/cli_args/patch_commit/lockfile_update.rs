use pnpm_lockfile::{Lockfile, SnapshotEntry};
use pnpm_package_manifest::PackageManifest;
use serde_json::Value;
use std::path::Path;

struct DeclaredDeps<'a> {
    deps: Option<&'a serde_json::Map<String, Value>>,
    opt_deps: Option<&'a serde_json::Map<String, Value>>,
    peer_deps: Option<&'a serde_json::Map<String, Value>>,
}

impl<'a> DeclaredDeps<'a> {
    fn from_manifest(manifest: &'a PackageManifest) -> Self {
        let val = manifest.value();
        Self {
            deps: val.get("dependencies").and_then(|v| v.as_object()),
            opt_deps: val.get("optionalDependencies").and_then(|v| v.as_object()),
            peer_deps: val.get("peerDependencies").and_then(|v| v.as_object()),
        }
    }
}

fn prune_snapshot_edges(snapshot: &mut SnapshotEntry, declared: &DeclaredDeps<'_>) -> bool {
    let mut changed = false;
    if let Some(deps) = &mut snapshot.dependencies {
        let initial_len = deps.len();
        deps.retain(|name, _| declared.deps.is_some_and(|d| d.contains_key(&name.to_string())));
        changed |= deps.len() != initial_len;
        if deps.is_empty() {
            snapshot.dependencies = None;
        }
    }
    if let Some(opt_deps) = &mut snapshot.optional_dependencies {
        let initial_len = opt_deps.len();
        opt_deps.retain(|name, _| {
            declared.opt_deps.is_some_and(|d| d.contains_key(&name.to_string()))
        });
        changed |= opt_deps.len() != initial_len;
        if opt_deps.is_empty() {
            snapshot.optional_dependencies = None;
        }
    }
    if let Some(trans_peers) = &mut snapshot.transitive_peer_dependencies {
        let initial_len = trans_peers.len();
        trans_peers.retain(|name| declared.peer_deps.is_some_and(|d| d.contains_key(name)));
        changed |= trans_peers.len() != initial_len;
        if trans_peers.is_empty() {
            snapshot.transitive_peer_dependencies = None;
        }
    }
    changed
}

fn prune_matching_snapshots(
    lockfile: &mut Lockfile,
    name: &str,
    version: &str,
    apply_to_all: bool,
    declared: &DeclaredDeps<'_>,
) -> bool {
    let Some(snapshots) = &mut lockfile.snapshots else {
        return false;
    };
    let mut changed = false;
    for (key, snapshot) in snapshots.iter_mut() {
        if key.name.to_string() == name
            && (apply_to_all || key.suffix.version().to_string() == version)
        {
            changed |= prune_snapshot_edges(snapshot, declared);
        }
    }
    changed
}

pub(super) fn update_lockfile_snapshots(
    workspace_dir: &Path,
    name: &str,
    version: &str,
    apply_to_all: bool,
    patched_manifest: &PackageManifest,
) -> Result<(), super::PatchCommitError> {
    let Some(mut lockfile) = Lockfile::load_wanted_from_dir(workspace_dir)
        .map_err(super::PatchCommitError::LoadLockfile)?
    else {
        return Ok(());
    };
    let declared = DeclaredDeps::from_manifest(patched_manifest);
    if !prune_matching_snapshots(&mut lockfile, name, version, apply_to_all, &declared) {
        return Ok(());
    }
    pnpm_package_manager::prune_unreachable_packages(&mut lockfile);
    let lockfile_path = workspace_dir.join("pnpm-lock.yaml");
    lockfile.save_to_path(&lockfile_path).map_err(super::PatchCommitError::SaveLockfile)
}

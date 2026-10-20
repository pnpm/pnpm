use crate::state::State;
use pnpm_lockfile::{Lockfile, SnapshotEntry};
use pnpm_package_manifest::PackageManifest;
use serde_json::Value;

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

    fn contains(&self, name: &str) -> bool {
        self.deps.is_some_and(|d| d.contains_key(name))
            || self.opt_deps.is_some_and(|d| d.contains_key(name))
            || self.peer_deps.is_some_and(|d| d.contains_key(name))
    }
}

fn prune_snapshot_edges(snapshot: &mut SnapshotEntry, declared: &DeclaredDeps<'_>) -> bool {
    let mut changed = false;
    if let Some(deps) = &mut snapshot.dependencies {
        let initial_len = deps.len();
        deps.retain(|name, _| declared.contains(&name.to_string()));
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
    state: &State,
    workspace_projects: &[pnpm_workspace::Project],
    name: &str,
    version: &str,
    apply_to_all: bool,
    patched_manifest: &PackageManifest,
) -> Result<(), super::PatchCommitError> {
    let declared = DeclaredDeps::from_manifest(patched_manifest);
    let selection = state.config.wanted_lockfile_selection();
    update_single_lockfile_snapshots(
        state.lockfile_dir(),
        &state.lockfile_path(),
        &selection,
        name,
        version,
        apply_to_all,
        &declared,
    )?;
    for project in workspace_projects {
        if project.root_dir == state.lockfile_dir() {
            continue;
        }
        let lockfile_dir = state.config.lockfile_dir_for(&project.root_dir);
        let lockfile_path = lockfile_dir.join(state.config.wanted_lockfile_name());
        update_single_lockfile_snapshots(
            lockfile_dir,
            &lockfile_path,
            &selection,
            name,
            version,
            apply_to_all,
            &declared,
        )?;
    }
    Ok(())
}

fn update_single_lockfile_snapshots(
    lockfile_dir: &std::path::Path,
    lockfile_path: &std::path::Path,
    selection: &pnpm_lockfile::WantedLockfileSelection,
    name: &str,
    version: &str,
    apply_to_all: bool,
    declared: &DeclaredDeps<'_>,
) -> Result<(), super::PatchCommitError> {
    let Some(mut lockfile) = Lockfile::load_wanted(lockfile_dir, selection)
        .map_err(super::PatchCommitError::LoadLockfile)?
    else {
        return Ok(());
    };
    if !prune_matching_snapshots(&mut lockfile, name, version, apply_to_all, declared) {
        return Ok(());
    }
    pnpm_package_manager::prune_unreachable_packages(&mut lockfile);
    lockfile.save_to_path(lockfile_path).map_err(super::PatchCommitError::SaveLockfile)
}

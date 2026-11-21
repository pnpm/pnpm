use crate::state::State;
use pnpm_lockfile::{
    Lockfile,
    PackageMetadata,
    SnapshotEntry,
    name_version_from_package_key,
};
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

    fn declares(&self, name: &str) -> bool {
        self.deps.is_some_and(|d| d.contains_key(name)) || self.declares_optional(name)
    }

    fn declares_optional(&self, name: &str) -> bool {
        self.opt_deps.is_some_and(|d| d.contains_key(name))
            || self.peer_deps.is_some_and(|d| d.contains_key(name))
    }
}

/// Whether the lockfile records `name` as a peer of the package. Peers are
/// resolved from the registry's manifest, which a patch does not change, so an
/// edge to one is kept even when the patched manifest drops it.
fn is_recorded_peer(metadata: Option<&PackageMetadata>, name: &str) -> bool {
    metadata.is_some_and(|metadata| {
        metadata.peer_dependencies
            .as_ref()
            .is_some_and(|peers| peers.contains_key(name))
            || metadata.peer_dependencies_meta
                .as_ref()
                .is_some_and(|meta| meta.contains_key(name))
    })
}

/// Drops the snapshot's edges that the patched manifest no longer declares.
/// A snapshot's `optionalDependencies` hold both its optional dependencies and
/// the optional peers it is bound to.
fn prune_snapshot_edges(
    snapshot: &mut SnapshotEntry,
    declared: &DeclaredDeps<'_>,
    metadata: Option<&PackageMetadata>,
) -> bool {
    let mut changed = false;
    if let Some(deps) = &mut snapshot.dependencies {
        let initial_len = deps.len();
        deps.retain(|name, _| {
            let name = name.to_string();
            declared.declares(&name) || is_recorded_peer(metadata, &name)
        });
        changed |= deps.len() != initial_len;
        if deps.is_empty() {
            snapshot.dependencies = None;
        }
    }
    if let Some(opt_deps) = &mut snapshot.optional_dependencies {
        let initial_len = opt_deps.len();
        opt_deps.retain(|name, _| {
            let name = name.to_string();
            declared.declares_optional(&name) || is_recorded_peer(metadata, &name)
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
    let packages = lockfile.packages.as_ref();
    for (key, snapshot) in snapshots.iter_mut() {
        let metadata_key = key.without_peer();
        let (key_name, key_version) = name_version_from_package_key(&metadata_key, packages);
        if key_name == name && (apply_to_all || key_version == version) {
            let metadata = packages.and_then(|packages| packages.get(&metadata_key));
            changed |= prune_snapshot_edges(snapshot, declared, metadata);
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
    update_single_lockfile_snapshots(
        state.lockfile_dir(),
        &state.lockfile_path(),
        state.config,
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
            state.config,
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
    config: &pnpm_config::Config,
    name: &str,
    version: &str,
    apply_to_all: bool,
    declared: &DeclaredDeps<'_>,
) -> Result<(), super::PatchCommitError> {
    let Some(mut lockfile) =
        Lockfile::load_wanted(lockfile_dir, &config.wanted_lockfile_selection())
            .map_err(super::PatchCommitError::LoadLockfile)?
    else {
        return Ok(());
    };
    if !prune_matching_snapshots(&mut lockfile, name, version, apply_to_all, declared) {
        return Ok(());
    }
    pnpm_package_manager::prune_unreachable_packages(&mut lockfile);
    lockfile
        .save_to_path(lockfile_path, config.symlinked_lockfile())
        .map_err(super::PatchCommitError::SaveLockfile)
}

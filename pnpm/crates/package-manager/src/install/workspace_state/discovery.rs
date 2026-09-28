use super::{
    super::{
        Catalogs, Config, IncludedDependencies, MaybeLazyLockfile, OptimisticRepeatInstallCheck,
        PackageManifest, Path, get_catalogs_from_workspace_manifest, load_workspace_projects,
    },
    build_project_manifests_list, configured_or_discovered_workspace_dir, lazy_wanted_lockfile,
    lockfile_root_for,
};
use std::borrow::Cow;

/// Discovery twin of [`install_already_up_to_date`](crate::install::workspace_state::install_already_up_to_date) for the
/// verify-deps-before-run gate: assemble the same
/// [`OptimisticRepeatInstallCheck`] inputs from a bare directory and run
/// [`crate::check_deps_status_before_run`].
///
/// Returns `None` when there is no manifest to check against. Outside a
/// workspace the run/exec command is about to fail with its own
/// missing-manifest error, and reporting "outdated" would only trigger
/// an install that can do nothing but crash with `NO_PKG_MANIFEST` (the
/// same guard pnpm's `checkDepsStatus` applies when it finds neither a
/// root manifest nor a workspace). Under dedicated per-project
/// lockfiles a directory with no manifest of its own owns no lockfile
/// and no state either.
///
/// Any other discovery failure conservatively reports the dependencies
/// as unverifiable ("Cannot check whether dependencies are outdated"),
/// matching pnpm's catch-all: in the worst case the configured action
/// runs a redundant install.
///
/// `selected_project_dirs` are the project directories the gated command
/// selected: a state that records a filtered install exempts the projects
/// that install did not select from the modules-directory requirement, but
/// the selected ones are still held to it.
#[must_use]
pub fn check_deps_status_before_run_at(
    dir: &Path,
    config: &Config,
    selected_project_dirs: &[&Path],
) -> Option<crate::RunDepsStatus> {
    let Ok(workspace_dir_opt) = configured_or_discovered_workspace_dir(config, dir) else {
        return cannot_check_deps();
    };
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| dir.to_path_buf());
    // One shared lockfile is written at the workspace root, whose
    // manifest heads the importer list the install recorded. Dedicated
    // per-project lockfiles give every project its own lockfile, state
    // and single-importer list, so the gate reads the manifest of the
    // project the command runs in.
    let manifest_dir = if config.shares_one_lockfile() { workspace_root.as_path() } else { dir };
    let manifest = match read_gate_manifest(
        manifest_dir,
        workspace_dir_opt.is_some(),
        config.shares_one_lockfile(),
    ) {
        GateManifest::Found(manifest) => manifest,
        GateManifest::NoManifest => return None,
        GateManifest::Unreadable => return cannot_check_deps(),
    };
    let Ok(workspace_manifest) = workspace_dir_opt
        .as_deref()
        .map(pnpm_workspace::read_workspace_manifest)
        .transpose()
    else {
        return cannot_check_deps();
    };
    let workspace_manifest = workspace_manifest.flatten();
    let config = gate_config(config, manifest_dir, &manifest);
    // A pinned `lockfileDir` is where the install left the state and the
    // lockfile; otherwise it follows the manifest read above, just as it
    // does during install.
    let lockfile_root = lockfile_root_for(&config, workspace_dir_opt.as_deref(), manifest_dir);
    // pnpm reports "cannot check" straight from the missing workspace
    // state, before any project discovery — a fresh project (the common
    // out-of-sync case) must not pay for the workspace-projects walk
    // only to reach the same verdict inside the check.
    deps_status_from_state(
        &GateDirs {
            dir,
            workspace_root: &workspace_root,
            lockfile_root: &lockfile_root,
            workspace_dir: workspace_dir_opt.as_deref(),
        },
        config.as_ref(),
        &manifest,
        workspace_manifest.as_ref(),
        selected_project_dirs,
    )
}

struct GateDirs<'a> {
    dir: &'a Path,
    workspace_root: &'a Path,
    lockfile_root: &'a Path,
    workspace_dir: Option<&'a Path>,
}

fn deps_status_from_state(
    dirs: &GateDirs<'_>,
    config: &Config,
    manifest: &PackageManifest,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    selected_project_dirs: &[&Path],
) -> Option<crate::RunDepsStatus> {
    match pnpm_workspace_state::load_workspace_state(dirs.lockfile_root) {
        Ok(Some(workspace_state)) => check_discovered_deps(
            config,
            manifest,
            workspace_manifest,
            dirs.workspace_root,
            dirs.lockfile_root,
            &workspace_state,
            selected_project_dirs,
        ),
        // No state file means this project was never installed on its own.
        // A dependency-free project an enclosing workspace leaves out has
        // nothing to install, and spawning one writes a lockfile into a
        // directory the workspace install left alone.
        Ok(None)
            if dependency_free_project_left_out(dirs.dir, config, dirs.workspace_dir, manifest) =>
        {
            Some(crate::RunDepsStatus::UpToDate)
        }
        _ => cannot_check_deps(),
    }
}

/// A project the enclosing workspace's `packages` patterns do not select,
/// whose manifest declares nothing an install would fetch.
///
/// Required `peerDependencies` count when `autoInstallPeers` is enabled,
/// because that install fetches them. Optional peers do not.
fn dependency_free_project_left_out(
    dir: &Path,
    config: &Config,
    workspace_dir: Option<&Path>,
    manifest: &PackageManifest,
) -> bool {
    if workspace_dir.is_some()
        || config.workspace_search_skipped
        || manifest_installs_dependencies(config, manifest)
    {
        return false;
    }
    pnpm_workspace::left_out_of_enclosing_workspace(dir).unwrap_or(false)
}

fn manifest_installs_dependencies(config: &Config, manifest: &PackageManifest) -> bool {
    crate::optimistic_repeat_install::manifest_has_runtime_deps(manifest)
        || (config.auto_install_peers && manifest_has_required_peers(manifest))
}

fn manifest_has_required_peers(manifest: &PackageManifest) -> bool {
    let Some(peers) = manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let meta = manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object);
    peers
        .keys()
        .any(|name| !peer_dependency_is_optional(meta, name))
}

fn peer_dependency_is_optional(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
    name: &str,
) -> bool {
    meta.and_then(|meta| meta.get(name))
        .and_then(serde_json::Value::as_object)
        .and_then(|entry| entry.get("optional"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// The directory the verify-deps-before-run gate serializes its installs
/// over: the workspace root, or `dir` outside a workspace. Every gate in one
/// workspace shares it, whichever project it runs in.
#[must_use]
pub fn deps_install_root(dir: &Path, config: &Config) -> std::path::PathBuf {
    configured_or_discovered_workspace_dir(config, dir)
        .ok()
        .flatten()
        .unwrap_or_else(|| dir.to_path_buf())
}
fn gate_config<'a>(
    config: &'a Config,
    manifest_dir: &Path,
    manifest: &PackageManifest,
) -> Cow<'a, Config> {
    if config.shares_one_lockfile() {
        Cow::Borrowed(config)
    } else {
        let mut project_config = config.clone();
        let project_name = manifest
            .value()
            .get("name")
            .and_then(serde_json::Value::as_str);
        project_config.anchor_dedicated_project(manifest_dir, project_name);
        Cow::Owned(project_config)
    }
}
pub(super) fn cannot_check_deps() -> Option<crate::RunDepsStatus> {
    Some(crate::RunDepsStatus::Outdated {
        issue: "Cannot check whether dependencies are outdated".to_string(),
        install_args: Vec::new(),
    })
}
pub(super) fn check_discovered_deps(
    config: &Config,
    manifest: &PackageManifest,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    workspace_root: &Path,
    lockfile_root: &Path,
    workspace_state: &pnpm_workspace_state::WorkspaceState,
    selected_project_dirs: &[&Path],
) -> Option<crate::RunDepsStatus> {
    let Some(catalogs) = configured_catalogs(config, workspace_manifest) else {
        return cannot_check_deps();
    };
    // The sibling projects only belong in the comparison when one
    // lockfile and one state file cover them all; a dedicated-lockfile
    // install records this project alone.
    let ignored_directories = config.managed_directories();
    let Ok(workspace_projects) = config
        .shares_one_lockfile()
        .then(|| load_workspace_projects(workspace_root, workspace_manifest, &ignored_directories))
        .transpose()
    else {
        return cannot_check_deps();
    };
    let workspace_projects = workspace_projects.flatten();
    let project_manifests = build_project_manifests_list(manifest, workspace_projects.as_deref());
    Some(crate::check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: lockfile_root,
            config,
            project_manifests: &project_manifests,
            is_workspace_install: workspace_manifest.is_some(),
            lockfile: MaybeLazyLockfile::Lazy(&lazy_wanted_lockfile(config, lockfile_root)),
            catalogs: &catalogs,
            layout: crate::RepeatInstallLayout {
                node_linker: config.node_linker,
                supported_architectures: config.supported_architectures.as_ref(),
                // The gate ignores dependency-group drift, so the groups only
                // shape the settings snapshot written back after a passing
                // content check — where the recorded values win anyway.
                included: IncludedDependencies {
                    dependencies: true,
                    dev_dependencies: true,
                    optional_dependencies: true,
                },
            },
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        workspace_state,
        selected_project_dirs,
    ))
}
/// The manifest the verify-deps gate compares against.
pub(super) enum GateManifest {
    Found(Box<PackageManifest>),
    /// No manifest to check against, so the gate has nothing to say.
    NoManifest,
    Unreadable,
}
pub(super) fn read_gate_manifest(
    manifest_dir: &Path,
    in_workspace: bool,
    shares_one_lockfile: bool,
) -> GateManifest {
    match pnpm_workspace::read_project_manifest_only(manifest_dir) {
        Ok(manifest) => GateManifest::Found(Box::new(manifest)),
        Err(pnpm_workspace::ReadProjectManifestOnlyError::NoImporterManifestFound { .. })
            if !in_workspace || !shares_one_lockfile =>
        {
            GateManifest::NoManifest
        }
        Err(_) => GateManifest::Unreadable,
    }
}
/// The catalogs in force: the configured ones, or the ones the workspace
/// manifest declares. `None` when the manifest's are unreadable.
pub(super) fn configured_catalogs(
    config: &Config,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Option<Catalogs> {
    match config.catalogs.clone() {
        Some(catalogs) => Some(catalogs),
        None => get_catalogs_from_workspace_manifest(workspace_manifest).ok(),
    }
}

use super::{
    super::{
        Catalogs, Config, IncludedDependencies, MaybeLazyLockfile, OptimisticRepeatInstallCheck,
        PackageManifest, Path, get_catalogs_from_workspace_manifest, load_workspace_projects,
    },
    build_project_manifests_list, configured_or_discovered_workspace_dir, lazy_wanted_lockfile,
    lockfile_root_for,
};
use std::{borrow::Cow, ffi::OsStr};

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
#[must_use]
pub fn check_deps_status_before_run_at(
    dir: &Path,
    config: &Config,
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
    let Ok(workspace_manifest) = gate_workspace_manifest(workspace_dir_opt.as_deref()) else {
        return cannot_check_deps();
    };
    let config = gate_config(config, manifest_dir, &manifest);
    // A pinned `lockfileDir` is where the install left the state and the
    // lockfile; otherwise it follows the manifest read above, just as it
    // does during install.
    let lockfile_root = lockfile_root_for(&config, workspace_dir_opt.as_deref(), manifest_dir);
    // pnpm reports "cannot check" straight from the missing workspace
    // state, before any project discovery — a fresh project (the common
    // out-of-sync case) must not pay for the workspace-projects walk
    // only to reach the same verdict inside the check.
    let Ok(Some(workspace_state)) = pnpm_workspace_state::load_workspace_state(&lockfile_root)
    else {
        return fallback_or_cannot_check(
            &config,
            &manifest,
            workspace_manifest.as_ref(),
            &workspace_root,
            &lockfile_root,
        );
    };
    check_discovered_deps(
        &config,
        &manifest,
        workspace_manifest.as_ref(),
        &workspace_root,
        &lockfile_root,
        &workspace_state,
    )
}

fn gate_workspace_manifest(
    workspace_dir_opt: Option<&Path>,
) -> Result<Option<pnpm_workspace::WorkspaceManifest>, ()> {
    workspace_dir_opt
        .map(pnpm_workspace::read_workspace_manifest)
        .transpose()
        .map(Option::flatten)
        .map_err(|_| ())
}

fn fallback_or_cannot_check(
    config: &Config,
    manifest: &PackageManifest,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    workspace_root: &Path,
    lockfile_root: &Path,
) -> Option<crate::RunDepsStatus> {
    installed_modules_match_lockfile(
        config,
        manifest,
        workspace_manifest,
        workspace_root,
        lockfile_root,
    )
    .then_some(crate::RunDepsStatus::UpToDate)
    .or_else(cannot_check_deps)
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

/// Whether the modules tree already records `lockfile_root`'s wanted lockfile.
///
/// The workspace state file is how the run gate usually knows that. When the
/// file is missing or unreadable, spawning an install opens the store index
/// and contacts the registry. This check reads the lockfiles and the project
/// manifests only.
fn modules_layout_valid(config: &Config) -> bool {
    let Ok(Some(modules)) =
        pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(&config.modules_dir)
    else {
        return false;
    };
    crate::install::modules_layout_satisfies_run(&modules, config, config.node_linker)
}

const ALL_DEPENDENCY_GROUPS: IncludedDependencies = IncludedDependencies {
    dependencies: true,
    dev_dependencies: true,
    optional_dependencies: true,
};

fn installed_modules_match_lockfile(
    config: &Config,
    manifest: &PackageManifest,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    workspace_root: &Path,
    lockfile_root: &Path,
) -> bool {
    if !modules_layout_valid(config) {
        return false;
    }
    let Ok(Some(wanted)) =
        pnpm_lockfile::Lockfile::load_wanted(lockfile_root, &config.wanted_lockfile_selection())
    else {
        return false;
    };
    let virtual_store = virtual_store_dir_for(config, lockfile_root);
    let Ok(Some(current)) =
        pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(&virtual_store)
    else {
        return false;
    };
    crate::optimistic_repeat_install::materialized_shape_matches(
        &wanted,
        &current,
        ALL_DEPENDENCY_GROUPS,
        config.peer_edge_options(),
    ) && manifests_match_lockfile(
        config,
        manifest,
        workspace_manifest,
        workspace_root,
        lockfile_root,
        &wanted,
    )
}

fn virtual_store_dir_for(config: &Config, lockfile_root: &Path) -> std::path::PathBuf {
    if config.explicit_settings.contains_key("virtualStoreDir")
        || config.enable_global_virtual_store
    {
        config.effective_virtual_store_dir().to_path_buf()
    } else if config.virtual_store_dir.starts_with(lockfile_root) {
        config.virtual_store_dir.clone()
    } else {
        let modules = config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
        lockfile_root.join(modules).join(".pnpm")
    }
}

fn load_projects(
    config: &Config,
    workspace_root: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Result<Option<Vec<pnpm_workspace::Project>>, ()> {
    let ignored = config.managed_directories();
    config
        .shares_one_lockfile()
        .then(|| load_workspace_projects(workspace_root, workspace_manifest, &ignored))
        .transpose()
        .map(Option::flatten)
        .map_err(|_| ())
}

fn manifests_match_lockfile(
    config: &Config,
    manifest: &PackageManifest,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    workspace_root: &Path,
    lockfile_root: &Path,
    wanted: &pnpm_lockfile::Lockfile,
) -> bool {
    let Ok(projects) = load_projects(config, workspace_root, workspace_manifest) else {
        return false;
    };
    let manifests = build_project_manifests_list(manifest, projects.as_deref());
    let Some(catalogs) = configured_catalogs(config, workspace_manifest) else {
        return false;
    };
    if !lockfile_settings_up_to_date(config, wanted, &catalogs) {
        return false;
    }
    let check = OptimisticRepeatInstallCheck {
        workspace_root: lockfile_root,
        config,
        project_manifests: &manifests,
        is_workspace_install: workspace_manifest.is_some(),
        lockfile: MaybeLazyLockfile::Loaded(Some(wanted)),
        catalogs: &catalogs,
        layout: crate::RepeatInstallLayout {
            node_linker: config.node_linker,
            supported_architectures: config.supported_architectures.as_ref(),
            included: ALL_DEPENDENCY_GROUPS,
        },
        manifest_freshness: crate::ManifestFreshness::Mtime,
    };
    crate::optimistic_repeat_install::first_project_missing_modules_dir(&check).is_none()
        && projects_satisfy_lockfile(config, lockfile_root, &manifests, wanted)
}

fn lockfile_settings_up_to_date(
    config: &Config,
    wanted: &pnpm_lockfile::Lockfile,
    catalogs: &Catalogs,
) -> bool {
    let Ok(parsed_overrides) = crate::install::parse_config_overrides(config, catalogs) else {
        return false;
    };
    crate::install::check_lockfile_settings_drift(
        wanted,
        config,
        catalogs,
        crate::install::CheckLockfileSettingsDriftOptions {
            parsed_overrides: parsed_overrides.as_deref(),
            pnpmfile_checksum: pnpm_lockfile::PnpmfileChecksumCheck::Skip,
            dedupe_peers: config.dedupe_peers,
        },
    )
    .is_ok()
}

fn projects_satisfy_lockfile(
    config: &Config,
    lockfile_root: &Path,
    manifests: &[(std::path::PathBuf, &PackageManifest)],
    wanted: &pnpm_lockfile::Lockfile,
) -> bool {
    let ignored_optional = pnpm_matcher::create_matcher(
        config.ignored_optional_dependencies.as_deref().unwrap_or_default(),
    );
    let is_ignored_optional = |name: &str| ignored_optional.matches(name);
    manifests
        .iter()
        .all(|(dir, manifest)| {
            let importer_id = if config.shares_one_lockfile() {
                pnpm_workspace::importer_id_from_root_dir(lockfile_root, dir)
            } else {
                ".".to_owned()
            };
            wanted.importers
                .get(&importer_id)
                .is_some_and(|importer| {
                    pnpm_lockfile::satisfies_package_manifest(
                        importer,
                        manifest,
                        config.auto_install_peers,
                        &is_ignored_optional,
                    )
                    .is_ok()
                })
        })
}

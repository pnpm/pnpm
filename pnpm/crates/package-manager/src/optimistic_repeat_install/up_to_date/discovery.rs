use super::{
    Catalogs, Config, IncludedDependencies, MaybeLazyLockfile, OptimisticRepeatInstallCheck,
    PackageManifest, Path, PathBuf, build_project_manifests_list,
    configured_or_discovered_workspace_dir, get_catalogs_from_workspace_manifest,
    lazy_wanted_lockfile, lockfile_root_for, nothing_to_install::projects_have_nothing_to_install,
    stand_ins,
};
use std::borrow::Cow;

/// Discovery twin of [`super::install_already_up_to_date`] for the
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
/// `workspace` is shared by the gates of one command, so under dedicated
/// per-project lockfiles each gate reads its projects instead of walking
/// the workspace again.
#[must_use]
pub fn check_deps_status_before_run_at(
    dir: &Path,
    config: &Config,
    selected_project_dirs: &[&Path],
    workspace: Option<&GateWorkspace>,
) -> Option<crate::RunDepsStatus> {
    let Ok(workspace_dir_opt) = configured_or_discovered_workspace_dir(config, dir) else {
        return cannot_check_deps();
    };
    let workspace_root = workspace_dir_opt.as_deref().unwrap_or(dir);
    // One shared lockfile is written at the workspace root, whose
    // manifest heads the importer list the install recorded. Dedicated
    // per-project lockfiles give every project its own lockfile, state
    // and single-importer list, so the gate reads the manifest of the
    // project the command runs in.
    let manifest_dir = if config.shares_one_lockfile() { workspace_root } else { dir };
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
    let config = gate_config(config, workspace_dir_opt.as_deref(), manifest_dir, &manifest);
    // A pinned `lockfileDir` is where the install left the state and the
    // lockfile; otherwise it follows the manifest read above, just as it
    // does during install.
    let lockfile_root = lockfile_root_for(&config, workspace_dir_opt.as_deref(), manifest_dir);
    deps_status_from_state(
        &GateInputs {
            config: &config,
            manifest: &manifest,
            workspace_manifest: workspace_manifest.as_ref(),
            workspace_root,
            lockfile_root: &lockfile_root,
            workspace,
        },
        selected_project_dirs,
    )
}

/// The workspace projects the verify-deps gate reads under dedicated
/// per-project lockfiles, for the gates of several projects in one command.
/// The first gate that needs them discovers them, and the others reuse them.
pub struct GateWorkspace {
    root: PathBuf,
    discovered: std::sync::OnceLock<Option<DiscoveredProjects>>,
}

/// One discovery of the workspace projects, with the workspace manifest and
/// the key of the options it ran with.
struct DiscoveredProjects {
    manifest: pnpm_workspace::WorkspaceManifest,
    key: pnpm_workspace::DiscoveryKey,
    projects: Vec<pnpm_workspace::Project>,
}

impl GateWorkspace {
    /// The workspace that `dir` belongs to. `None` outside a workspace.
    #[must_use]
    pub fn of(dir: &Path, config: &Config) -> Option<Self> {
        let root = configured_or_discovered_workspace_dir(config, dir).ok()??;
        Some(Self { root, discovered: std::sync::OnceLock::new() })
    }

    /// Drop the workspace manifest and the discovered projects, so the next
    /// gate reads them again after an install that may have changed them.
    pub fn forget_projects(&mut self) {
        self.discovered.take();
    }

    /// Its projects, as [`check_deps_status_before_run_at`] would discover
    /// them under `config`. `None` when the discovery failed, or when
    /// `config` discovers with other options than the gate that ran it, such
    /// as a project's own `modulesDir`.
    fn projects(&self, config: &Config) -> Option<&[pnpm_workspace::Project]> {
        let discovered = self.discovered
            .get_or_init(|| {
                let manifest = pnpm_workspace::read_workspace_manifest(&self.root).ok()??;
                let opts = crate::install::checked_projects_opts(config, &manifest);
                let projects = pnpm_workspace::find_workspace_projects(&self.root, &opts).ok()?;
                let key = opts.discovery_key(&self.root);
                Some(DiscoveredProjects { manifest, key, projects })
            })
            .as_ref()?;
        let key = crate::install::checked_projects_opts(config, &discovered.manifest)
            .discovery_key(&self.root);
        (discovered.key == key).then_some(discovered.projects.as_slice())
    }
}

/// What the verify-deps gate read before it consults the workspace state.
#[derive(Clone, Copy)]
pub(super) struct GateInputs<'a> {
    pub(super) config: &'a Config,
    pub(super) manifest: &'a PackageManifest,
    pub(super) workspace_manifest: Option<&'a pnpm_workspace::WorkspaceManifest>,
    pub(super) workspace_root: &'a Path,
    pub(super) lockfile_root: &'a Path,
    /// See [`check_deps_status_before_run_at`].
    pub(super) workspace: Option<&'a GateWorkspace>,
}

/// pnpm reports "cannot check" straight from the missing workspace state,
/// before any project discovery — a fresh project (the common out-of-sync
/// case) must not pay for the workspace-projects walk only to reach the
/// same verdict inside the check. Projects are walked only when the
/// manifest itself gives an install nothing to do.
fn deps_status_from_state(
    inputs: &GateInputs<'_>,
    selected_project_dirs: &[&Path],
) -> Option<crate::RunDepsStatus> {
    if inputs.config.node_linker == pnpm_config::NodeLinker::Loaded
        && [pnpm_config::CAS_MANIFEST_FILENAME, pnpm_config::CAS_LOADER_FILENAME]
            .iter()
            .any(|filename| {
                !inputs.config
                    .store_loader_dir(inputs.lockfile_root)
                    .join(filename)
                    .is_file()
            })
    {
        return cannot_check_deps();
    }
    match pnpm_workspace_state::load_workspace_state_in_modules(
        &inputs.config.workspace_state_modules_dir(inputs.lockfile_root),
    ) {
        Ok(Some(workspace_state)) => {
            check_discovered_deps(inputs, &workspace_state, selected_project_dirs)
        }
        // Nothing was installed here yet. Spawning an install for projects
        // that give it nothing to do would only leave a lockfile and
        // `node_modules` behind.
        Ok(None) if projects_have_nothing_to_install(inputs) => {
            Some(crate::RunDepsStatus::UpToDate)
        }
        _ => cannot_check_deps(),
    }
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
    workspace_dir: Option<&Path>,
    manifest_dir: &Path,
    manifest: &PackageManifest,
) -> Cow<'a, Config> {
    let needs_workspace_dir = config.workspace_dir.is_none() && workspace_dir.is_some();
    if config.shares_one_lockfile() && !needs_workspace_dir {
        return Cow::Borrowed(config);
    }
    let mut project_config = config.clone();
    if needs_workspace_dir {
        project_config.workspace_dir = workspace_dir.map(Path::to_path_buf);
    }
    if !config.shares_one_lockfile() {
        let project_name = manifest
            .value()
            .get("name")
            .and_then(serde_json::Value::as_str);
        project_config.anchor_dedicated_project(manifest_dir, project_name);
    }
    Cow::Owned(project_config)
}
pub(super) fn cannot_check_deps() -> Option<crate::RunDepsStatus> {
    Some(crate::RunDepsStatus::Outdated {
        issue: "Cannot check whether dependencies are outdated".to_string(),
        install_args: Vec::new(),
    })
}
pub(super) fn check_discovered_deps(
    inputs: &GateInputs<'_>,
    workspace_state: &pnpm_workspace_state::WorkspaceState,
    selected_project_dirs: &[&Path],
) -> Option<crate::RunDepsStatus> {
    let &GateInputs {
        config,
        manifest,
        workspace_manifest,
        workspace_root,
        lockfile_root,
        ..
    } = inputs;
    let Some(catalogs) = configured_catalogs(config, workspace_manifest) else {
        return cannot_check_deps();
    };
    let gate = GateCheck {
        config,
        workspace_manifest,
        lockfile_root,
        catalogs: &catalogs,
        state: workspace_state,
        selected_project_dirs,
    };
    let listed = match gate_from_records(&gate, manifest, workspace_root) {
        Ok(()) => return Some(crate::RunDepsStatus::UpToDate),
        Err(listed) => listed,
    };
    let mut loaded = None;
    let Ok(workspace_projects) = gate_workspace_projects(inputs, listed, &mut loaded) else {
        return cannot_check_deps();
    };
    let (project_manifests, bin_projects) = gated_projects(config, manifest, workspace_projects);
    Some(run_gate(
        &gate,
        crate::DeclaredManifests::new(&project_manifests),
        bin_projects.as_deref(),
    ))
}

/// What every run of the verify-deps check in one gate shares.
struct GateCheck<'a> {
    config: &'a Config,
    workspace_manifest: Option<&'a pnpm_workspace::WorkspaceManifest>,
    lockfile_root: &'a Path,
    catalogs: &'a Catalogs,
    state: &'a pnpm_workspace_state::WorkspaceState,
    selected_project_dirs: &'a [&'a Path],
}

/// The gate on manifests rebuilt from the last validation's records, per
/// [`stand_ins::list_stand_ins`], when one lockfile covers the workspace.
/// `Ok` when it finds the dependencies up to date. Any other verdict is left
/// to the gate on the files, so what the gate reports comes from them. `Err`
/// holds the listing that gate reads, when one was made.
fn gate_from_records(
    gate: &GateCheck<'_>,
    manifest: &PackageManifest,
    workspace_root: &Path,
) -> Result<(), Option<Vec<pnpm_workspace::ProjectManifestPaths>>> {
    if !gate.config.shares_one_lockfile() {
        return Err(None);
    }
    let listing = stand_ins::list_stand_ins(&stand_ins::ListedWorkspace {
        config: gate.config,
        workspace_root,
        workspace_manifest: gate.workspace_manifest,
        state_root: gate.lockfile_root,
        state: gate.state,
    });
    let (listed, stand_ins) = match listing {
        stand_ins::Listing::StandIns { listed, stand_ins } => (listed, stand_ins),
        stand_ins::Listing::Files(listed) => return Err(listed),
    };
    let project_manifests = build_project_manifests_list(manifest, Some(&stand_ins.projects));
    let recorded_digests = stand_ins::stand_in_digests(&project_manifests, &stand_ins, manifest);
    let declared = crate::DeclaredManifests::with_stand_ins(&project_manifests, &recorded_digests);
    if run_gate(gate, declared, None) == crate::RunDepsStatus::UpToDate {
        tracing::debug!(target: "pacquet::run", "the recorded manifests decided the dependencies");
        return Ok(());
    }
    tracing::debug!(target: "pacquet::run", "the recorded manifests cannot decide the dependencies");
    Err(Some(listed))
}

/// The workspace projects the gate reads, `None` when it reads none. Under
/// dedicated lockfiles the projects only feed the bins, so the ones the
/// command discovered stand in for a walk of the workspace. A walk leaves
/// its projects in `loaded`.
fn gate_workspace_projects<'a>(
    inputs: &GateInputs<'a>,
    listed: Option<Vec<pnpm_workspace::ProjectManifestPaths>>,
    loaded: &'a mut Option<Vec<pnpm_workspace::Project>>,
) -> Result<Option<&'a [pnpm_workspace::Project]>, pnpm_workspace::FindWorkspaceProjectsError> {
    let &GateInputs {
        config,
        manifest,
        workspace_manifest,
        workspace_root,
        workspace,
        ..
    } = inputs;
    let links = crate::install_state::validation::links_workspace_commands(manifest);
    let discovered = workspace
        .filter(|workspace| workspace.root == workspace_root)
        .filter(|_| !config.shares_one_lockfile() && links)
        .and_then(|workspace| workspace.projects(config));
    if discovered.is_some() {
        return Ok(discovered);
    }
    *loaded = load_gated_workspace(config, manifest, workspace_root, workspace_manifest, listed)?;
    Ok(loaded.as_deref())
}

/// [`crate::check_deps_status_before_run`] on `project_manifests`.
fn run_gate(
    gate: &GateCheck<'_>,
    project_manifests: crate::DeclaredManifests<'_>,
    bin_projects: Option<&[(PathBuf, &PackageManifest)]>,
) -> crate::RunDepsStatus {
    crate::check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: gate.lockfile_root,
            config: gate.config,
            project_manifests,
            is_workspace_install: gate.workspace_manifest.is_some(),
            lockfile: MaybeLazyLockfile::Lazy(&lazy_wanted_lockfile(
                gate.config,
                gate.lockfile_root,
            )),
            catalogs: gate.catalogs,
            layout: crate::RepeatInstallLayout {
                node_linker: gate.config.node_linker,
                supported_architectures: gate.config.supported_architectures.as_ref(),
                // The gate ignores dependency-group drift, so the groups only
                // shape the settings snapshot written back after a passing
                // content check — where the recorded values win anyway.
                included: IncludedDependencies {
                    dependencies: true,
                    dev_dependencies: true,
                    optional_dependencies: true,
                },
            },
            manifests: crate::RepeatInstallManifests {
                workspace_projects: bin_projects,
                ..crate::RepeatInstallManifests::ON_DISK
            },
        },
        gate.state,
        gate.selected_project_dirs,
    )
}

/// The workspace projects the gate reads: every one when one lockfile and one
/// state file cover them all, and for a dedicated-lockfile install the ones
/// whose commands the project links, when it links any.
///
/// `listed` names the projects to read, when the caller already listed them.
fn load_gated_workspace(
    config: &Config,
    manifest: &PackageManifest,
    workspace_root: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    listed: Option<Vec<pnpm_workspace::ProjectManifestPaths>>,
) -> Result<Option<Vec<pnpm_workspace::Project>>, pnpm_workspace::FindWorkspaceProjectsError> {
    let needed = config.shares_one_lockfile()
        || crate::install_state::validation::links_workspace_commands(manifest);
    if !needed {
        return Ok(None);
    }
    match listed {
        Some(listed) => pnpm_workspace::read_workspace_projects(listed).map(Some),
        None => crate::install::load_checked_workspace_projects(
            config,
            workspace_root,
            workspace_manifest,
        ),
    }
}

/// The projects the gate compares, and the ones whose commands they link per
/// [`crate::RepeatInstallManifests::workspace_projects`].
#[expect(
    clippy::type_complexity,
    reason = "the two lists are the ones `OptimisticRepeatInstallCheck` takes"
)]
fn gated_projects<'a>(
    config: &Config,
    manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
) -> (Vec<(PathBuf, &'a PackageManifest)>, Option<Vec<(PathBuf, &'a PackageManifest)>>) {
    let shared = workspace_projects.filter(|_| config.shares_one_lockfile());
    let project_manifests = build_project_manifests_list(manifest, shared);
    let bin_projects = crate::install_state::validation::dedicated_bin_projects(
        config,
        &project_manifests,
        workspace_projects,
    );
    (project_manifests, bin_projects)
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
        // A workspace root without a manifest is no importer, as the install
        // treats it, and the workspace projects are checked without it.
        Err(pnpm_workspace::ReadProjectManifestOnlyError::NoImporterManifestFound { .. }) => {
            GateManifest::Found(Box::new(PackageManifest::from_value(
                manifest_dir.join("package.json"),
                serde_json::Value::Object(serde_json::Map::new()),
            )))
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
    match &config.catalogs {
        Some(catalogs) => Some(catalogs.clone()),
        None => get_catalogs_from_workspace_manifest(workspace_manifest).ok(),
    }
}

#[cfg(test)]
mod tests;

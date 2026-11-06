//! The repeat-install check a command runs on its own, before it sets up an
//! install: the `pnpm install` shortcut the CLI takes before starting the
//! async runtime, and the verify-deps gate before `pnpm run` and `pnpm exec`.

pub use discovery::{GateWorkspace, check_deps_status_before_run_at, deps_install_root};
pub use per_project::install_already_up_to_date_per_project;

mod discovery;

mod from_files;

mod per_project;

mod stand_ins;

mod nothing_to_install;

use super::{check_optimistic_repeat_install_with_state, config_blocks_fast_path};
use crate::{
    install::{
        build_project_manifests_list, check_strict_dep_builds_for_fast_path,
        configured_or_discovered_workspace_dir, effective_node_version, gvs_build_markers_complete,
        included_dependencies, load_workspace_projects, lockfile_root_for,
        project_requires_lifecycle_scripts, register_workspace_in_store,
    },
    install_state::{Decision, OptimisticRepeatInstallCheck, Scope},
};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, NodeLinker};
use pnpm_executor::PROJECT_LIFECYCLE_STAGES;
use pnpm_lockfile::{LazyLockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_is_installable::PackageInstallabilityManifest;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_workspace_state::{WorkspaceState, load_workspace_state};
use std::path::{Path, PathBuf};

/// Inputs for [`install_already_up_to_date`].
pub struct UpToDateFastPathCheck<'a> {
    pub config: &'a Config,
    pub manifest: &'a PackageManifest,
    pub dependency_groups: Vec<DependencyGroup>,
    pub node_linker: NodeLinker,
    /// The CLI-merged effective `supportedArchitectures` (yaml plus
    /// `--cpu` / `--os` / `--libc`) — the fast path must not report
    /// "Already up to date" when a flag changed the target platforms.
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
}

/// What an up-to-date install reports about the workspace it covered.
#[derive(Debug)]
pub struct UpToDateWorkspace {
    /// The workspace root — the reporter `prefix` for the "Already up to
    /// date" emission.
    pub root: PathBuf,
    /// How many projects the workspace has, or `None` when there is no
    /// `pnpm-workspace.yaml`. Feeds the `pnpm:scope` report, which the
    /// full install path derives from the same walk.
    pub project_count: Option<usize>,
    /// What each covered project requires of the host, per
    /// [`pnpm_deps_restorer::project_host_requirements`], by project
    /// directory. A project that requires nothing is left out.
    pub host_requirements: Vec<(PathBuf, PackageInstallabilityManifest)>,
}

/// The projects a check found up to date.
struct CoveredProjects {
    /// See [`UpToDateWorkspace::project_count`].
    count: Option<usize>,
    /// See [`UpToDateWorkspace::host_requirements`].
    host_requirements: Vec<(PathBuf, PackageInstallabilityManifest)>,
}

impl CoveredProjects {
    fn new<'a>(
        count: Option<usize>,
        manifests: impl IntoIterator<Item = (&'a Path, &'a PackageManifest)>,
    ) -> Self {
        let host_requirements = manifests
            .into_iter()
            .filter_map(|(root_dir, manifest)| {
                pnpm_deps_restorer::project_host_requirements(manifest.value())
                    .map(|requirements| (root_dir.to_path_buf(), requirements))
            })
            .collect();
        CoveredProjects { count, host_requirements }
    }

    fn into_workspace(self, root: PathBuf) -> UpToDateWorkspace {
        UpToDateWorkspace {
            root,
            project_count: self.count,
            host_requirements: self.host_requirements,
        }
    }

    fn of(count: Option<usize>, manifests: &[(PathBuf, &PackageManifest)]) -> Self {
        Self::new(
            count,
            manifests
                .iter()
                .map(|(root_dir, manifest)| (root_dir.as_path(), *manifest)),
        )
    }
}

/// [`crate::check_optimistic_repeat_install`] inputs, callable from a
/// synchronous context so the CLI can finish an up-to-date install
/// before paying for the async runtime, the HTTP client, and the
/// state setup. Returns what the short-circuit covered when the install
/// can take it, after registering the workspace in the store as
/// [`crate::Install::run`] does.
///
/// `Err` carries the reason the shortcut was not taken, for the
/// `pacquet::install` trace. Failures deliberately collapse into it: the
/// caller falls through to the full install path, which reproduces the
/// failure with its established error shape.
///
/// The phases that read the project tree run inside `pacquet::install`
/// spans, so a trace of a slow up-to-date install shows which of them took
/// the time.
pub fn install_already_up_to_date(
    check: &UpToDateFastPathCheck<'_>,
) -> Result<UpToDateWorkspace, &'static str> {
    already_up_to_date(check, Decide::FromRecordsOrFiles)
}

/// [`install_already_up_to_date`] decided from the manifest records alone. A
/// caller whose install reads the project manifests anyway when the
/// shortcut is not taken uses this, so a declined attempt reads none of them.
pub fn install_already_up_to_date_from_records(
    check: &UpToDateFastPathCheck<'_>,
) -> Result<UpToDateWorkspace, &'static str> {
    already_up_to_date(check, Decide::FromRecords)
}

/// What an up-to-date verdict may be decided from.
#[derive(Clone, Copy)]
enum Decide {
    FromRecords,
    FromRecordsOrFiles,
}

fn already_up_to_date(
    check: &UpToDateFastPathCheck<'_>,
    decide: Decide,
) -> Result<UpToDateWorkspace, &'static str> {
    let manifest_dir = check.manifest
        .path()
        .parent()
        .ok_or("the manifest has no directory")?;
    let workspace_dir_opt = configured_or_discovered_workspace_dir(check.config, manifest_dir)
        .map_err(|_| "the workspace root cannot be located")?;
    let workspace_root = workspace_dir_opt.as_deref().unwrap_or(manifest_dir);
    // The lockfile the install wrote sits at its `lockfileDir`, which
    // both `sharedWorkspaceLockfile: false` and an explicit pin move away
    // from the discovered workspace root. The workspace *state* keeps its
    // own root, which only a pin moves — `state_root` below.
    let lockfile_root = lockfile_root_for(check.config, workspace_dir_opt.as_deref(), manifest_dir);
    let state_root = check.config.lockfile_dir.as_deref().unwrap_or(workspace_root);
    let state = load_fast_path_state(check.config, &lockfile_root, state_root)?;
    let (workspace_manifest, catalogs) =
        fast_path_workspace_context(check.config, workspace_dir_opt.as_deref())
            .ok_or("the workspace manifest or its catalogs cannot be read")?;
    let lockfile = lazy_wanted_lockfile(check.config, &lockfile_root);
    let context = FastPathContext {
        state_root,
        lockfile_root: &lockfile_root,
        workspace_root,
        workspace_manifest: workspace_manifest.as_ref(),
        lockfile: &lockfile,
        catalogs: &catalogs,
    };
    let covered = match stand_ins::check_from_records(check, &context, &state) {
        stand_ins::RecordsCheck::UpToDate(covered) => covered,
        stand_ins::RecordsCheck::Undecided(listed) => match decide {
            Decide::FromRecordsOrFiles => {
                from_files::check_from_files(check, &context, &state, listed)?
            }
            Decide::FromRecords => return Err("the recorded manifests cannot decide"),
        },
    };
    register_fast_path_workspace(check.config, &lockfile_root)?;
    Ok(covered.into_workspace(state_root.to_path_buf()))
}

fn load_fast_path_state(
    config: &Config,
    lockfile_root: &Path,
    state_root: &Path,
) -> Result<WorkspaceState, &'static str> {
    if let Some(reason) = config_blocks_fast_path(config) {
        return Err(reason);
    }
    if !matches!(check_strict_dep_builds_for_fast_path(config), Ok(true)) {
        return Err("strictDepBuilds has to re-check the recorded ignored builds");
    }
    if crate::agent_skills::agent_skill_targets_changed(config, lockfile_root) {
        return Err("an agent skill directory changed");
    }
    load_workspace_state(state_root)
        .ok()
        .flatten()
        .ok_or("no workspace state on disk")
}

/// What [`install_already_up_to_date`] has established about the workspace
/// before it reads any project.
struct FastPathContext<'a> {
    state_root: &'a Path,
    lockfile_root: &'a Path,
    workspace_root: &'a Path,
    /// `None` when no `pnpm-workspace.yaml` drives the install.
    workspace_manifest: Option<&'a pnpm_workspace::WorkspaceManifest>,
    lockfile: &'a LazyLockfile,
    catalogs: &'a Catalogs,
}

/// The projects one repeat-install check covers.
#[derive(Clone, Copy)]
struct FastPathProjects<'a> {
    state: &'a WorkspaceState,
    manifests: &'a [(PathBuf, &'a PackageManifest)],
    /// See [`crate::DeclaredManifests::with_stand_ins`].
    recorded_digests: Option<&'a std::collections::HashMap<PathBuf, String>>,
    /// See [`crate::RepeatInstallManifests::workspace_projects`].
    bin_projects: Option<&'a [(PathBuf, &'a PackageManifest)]>,
}

/// [`check_optimistic_repeat_install_with_state`] inside the
/// `check_repeat_install` span, with a skip surfaced as the fast path's reason.
fn check_fast_path_repeat_install(
    check: &UpToDateFastPathCheck<'_>,
    context: &FastPathContext<'_>,
    projects: FastPathProjects<'_>,
) -> Result<(), &'static str> {
    let _span = tracing::debug_span!(target: "pacquet::install", "check_repeat_install").entered();
    let project_manifests = match projects.recorded_digests {
        Some(digests) => crate::DeclaredManifests::with_stand_ins(projects.manifests, digests),
        None => crate::DeclaredManifests::new(projects.manifests),
    };
    let decision = check_optimistic_repeat_install_with_state(
        &OptimisticRepeatInstallCheck {
            workspace_root: context.state_root,
            config: check.config,
            project_manifests,
            is_workspace_install: context.workspace_manifest.is_some(),
            lockfile: MaybeLazyLockfile::Lazy(context.lockfile),
            catalogs: context.catalogs,
            layout: crate::RepeatInstallLayout {
                node_linker: check.node_linker,
                included: included_dependencies(&check.dependency_groups),
                supported_architectures: check.supported_architectures.as_ref(),
            },
            manifests: crate::RepeatInstallManifests {
                workspace_projects: projects.bin_projects,
                ..crate::RepeatInstallManifests::ON_DISK
            },
        },
        projects.state,
        Scope::Workspace,
    );
    match decision {
        Decision::UpToDate => gvs_build_markers_complete(
            check.config,
            MaybeLazyLockfile::Lazy(context.lockfile),
            context.lockfile_root,
            effective_node_version(check.config, check.manifest).as_deref(),
        ),
        Decision::Skipped { reason } => Err(reason),
    }
}

/// [`register_workspace_in_store`] inside the
/// `register_workspace_in_store` span.
fn register_fast_path_workspace(config: &Config, lockfile_root: &Path) -> Result<(), &'static str> {
    let _span =
        tracing::debug_span!(target: "pacquet::install", "register_workspace_in_store").entered();
    register_workspace_in_store(config, lockfile_root)
        .map_err(|_| "the workspace cannot be registered in the store")
}

fn fast_path_workspace_context(
    config: &Config,
    workspace_dir: Option<&Path>,
) -> Option<(Option<pnpm_workspace::WorkspaceManifest>, Catalogs)> {
    let workspace_manifest = workspace_dir
        .map(pnpm_workspace::read_workspace_manifest)
        .transpose()
        .ok()?
        .flatten();
    let catalogs = discovery::configured_catalogs(config, workspace_manifest.as_ref())?;
    Some((workspace_manifest, catalogs))
}

/// The wanted lockfile, read on first use — or a stand-in that never reads
/// one when the install is configured without a lockfile.
fn lazy_wanted_lockfile(config: &Config, lockfile_root: &Path) -> LazyLockfile {
    if config.lockfile {
        LazyLockfile::deferred(lockfile_root.to_path_buf(), config.wanted_lockfile_selection())
    } else {
        LazyLockfile::disabled()
    }
}

pub(crate) mod state_options;

pub use entry_points::apply_deploy_manifest_hook;
pub(crate) use entry_points::apply_deploy_manifest_hook_to_arc;
pub use errors::{InstallError, defer_ignored_builds};
pub(crate) use lockfile_freshness::{
    CheckLockfileSettingsDriftOptions, FreshnessCheckError, FreshnessScope,
    ImporterSatisfactionCheck, OptionalDependencyExclusions, check_importer_satisfies,
    check_lockfile_settings_drift, parse_config_overrides,
};
pub use lockfile_freshness::{
    WantedLockfileSatisfactionCheck, wanted_lockfile_satisfies_workspace,
};
pub(crate) use modules_state::{
    frozen_tree_intact, hoisted_workspace_packages_present, modules_layout_consistent_with,
    moved_tree_is_reusable, tree_may_move,
};
pub use run::{InstallExecution, InstallLockfilePolicy, ResolutionInputs};
pub use workspace_state::{
    UpToDateFastPathCheck, UpToDateWorkspace, build_workspace_packages_map,
    check_deps_status_before_run_at, install_already_up_to_date,
};
pub(crate) use workspace_state::{
    build_workspace_state, configured_or_discovered_workspace_dir, lockfile_root_dir,
    workspace_packages_for_freshness,
};

mod entry_points;

mod errors;

use errors::{map_fresh_lockfile_error, map_frozen_lockfile_error};

use crate::{
    HoistedDependencies, InstallFrozenLockfile, InstallWithFreshLockfile,
    InstallWithFreshLockfileError, LockfileVerificationOverride, OptimisticRepeatInstallCheck,
    RebuildOptions, ResolvedPackages, UpdateSeedPolicy, build_resolution_verifiers,
    check_optimistic_repeat_install, emit_initial_package_manifest, link_project_bins,
    optimistic_repeat_install::Decision as OptimisticRepeatInstallDecision,
    prune_merged_branch_lockfile::prune_merged_branch_lockfile, report_merged_lockfile_conflicts,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, NodeLinker, PNPM_VERSION};
use pnpm_executor::{
    DEV_PREINSTALL_ALREADY_RAN_ENV, PROJECT_INSTALL_STAGES, PROJECT_LIFECYCLE_STAGES,
    PROJECT_POST_UNINSTALL_STAGES, PROJECT_PRE_UNINSTALL_STAGES, ROOT_PREINSTALL_ALREADY_RAN_ENV,
    RunPostinstallHooks, run_project_lifecycle_stages,
};
use pnpm_lockfile::{
    LazyLockfile, Lockfile, LockfileEntries, MaybeLazyLockfile, PnpmfileChecksumCheck,
    StalenessReason, VersionPart, satisfies_package_manifest,
};
use pnpm_lockfile_verification::{
    VerifyLockfileResolutionsOptions, record_lockfile_verified, verify_lockfile_resolutions,
};
use pnpm_modules_yaml::{
    Clock, Host, IncludedDependencies, LayoutVersion, Modules, NodeLinker as ModulesNodeLinker,
    write_modules_manifest,
};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_package_manifest::{DependencyGroup, PackageManifest, node_version_from_engines_runtime};
use pnpm_reporter::{
    ContextLog, GlobalLog, LogEvent, LogLevel, PnpmLog, Reporter, ScopeLog, Stage, StageLog,
    SummaryLog,
};
use pnpm_resolving_npm_resolver::InMemoryPackageMetaCache;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_tarball::MemCache;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState, update_workspace_state};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
    time::SystemTime,
};

mod apply_materialization;
mod lifecycle;
mod lockfile_freshness;
mod materialize;
mod modules_state;
mod prepare_modules_state;
mod time_machine;
/// The dependency groups the install includes, as `.modules.yaml` records
/// them and the dependency-graph walker observes them.
pub(super) fn included_dependencies(dependency_groups: &[DependencyGroup]) -> IncludedDependencies {
    IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    }
}

mod run;
mod workspace_state;

use apply_materialization::{ApplyMaterializationInputs, apply_materialization_result};
use lifecycle::{
    dev_preinstall_already_ran, load_workspace_projects, project_lifecycle_graph,
    project_script_stages, root_preinstall_already_ran, run_pre_uninstall_scripts,
    run_projects_lifecycle_scripts, run_root_hook,
};
use lockfile_freshness::{
    FastUpdateLockfileOptions, check_lockfile_freshness, try_fast_update_lockfile,
};
use materialize::{MaterializationInputs, Materialized, materialize};
use modules_state::{
    build_modules_manifest, check_modules_settings_diff, current_contains_dep_path,
    drain_settled_projects, gvs_build_marker_present, gvs_build_markers_may_require_recovery,
    has_newly_allowed_ignored_builds, manifest_string_field, merge_filtered_modules_metadata,
    merge_pending_builds, modules_consistent_with, project_requires_lifecycle_scripts,
    recorded_allow_builds_differ, unapproved_recorded_ignored_builds,
};
use prepare_modules_state::{
    PrepareModulesStateInputs, PreparedModulesState, prepare_modules_state,
    prior_hoisted_dependencies, prior_hoisted_locations,
};
use time_machine::TimeMachineExclusions;
use workspace_state::{
    ProjectScriptsInputs, build_project_manifests_list, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, lockfile_root_for, mutated_project_dirs,
    projects_running_own_scripts, selected_manifest_freshness_inputs,
};

#[cfg(test)]
mod tests;

/// Run the lockfile verification fan-out to completion, blocking the
/// caller on the verdict. Used by the install paths that have no fetch
/// to overlap verification with (fresh resolve, the lockfile-only and
/// up-to-date short-circuits); the frozen materialization path instead
/// runs verification concurrently with the fetch inside
/// [`InstallFrozenLockfile`]. A no-op when `verifiers` is empty.
async fn verify_lockfile_eagerly<Reporter: pnpm_reporter::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<(), InstallError> {
    if verifiers.is_empty() {
        return Ok(());
    }
    verify_lockfile_resolutions::<Reporter>(
        lockfile,
        verifiers,
        &VerifyLockfileResolutionsOptions {
            concurrency: None,
            lockfile_path,
            cache_dir: Some(cache_dir),
        },
    )
    .await
    .map_err(InstallError::LockfileVerification)
}

/// The pre-resolve lockfile-verification fan-out, spawned so its
/// registry round trips overlap the fresh path's resolve and
/// materialization instead of serializing in front of them — the same
/// concurrent-gate contract the frozen path's `select!` provides. The
/// verdict still gates everything sensitive:
/// [`InstallWithFreshLockfile`] awaits the gate before bin linking,
/// dependency builds, and the lockfile save.
///
/// Aborts the fan-out on drop so an install that fails before reaching
/// the gate doesn't leave verification requests running in the host
/// process (the napi embedding outlives a failed install).
pub struct LockfileVerificationGate(
    tokio::task::JoinHandle<Result<(), pnpm_lockfile_verification::VerifyError>>,
);

pub(crate) fn untracked_read_package_hook_may_have_changed(
    recorded: Option<bool>,
    current: Option<bool>,
) -> bool {
    current == Some(true) || recorded != current
}

impl LockfileVerificationGate {
    /// Start the fan-out in the background, or `None` when no verifier
    /// is active (`trustLockfile`).
    fn spawn<Reporter: pnpm_reporter::Reporter + Send + 'static>(
        lockfile: &Lockfile,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        lockfile_path: Option<&Path>,
        cache_dir: &Path,
    ) -> Option<Self> {
        if verifiers.is_empty() {
            return None;
        }
        let lockfile = lockfile.clone();
        let verifiers = verifiers.to_vec();
        let lockfile_path = lockfile_path.map(Path::to_path_buf);
        let cache_dir = cache_dir.to_path_buf();
        Some(Self(tokio::spawn(async move {
            verify_lockfile_resolutions::<Reporter>(
                &lockfile,
                &verifiers,
                &VerifyLockfileResolutionsOptions {
                    concurrency: None,
                    lockfile_path: lockfile_path.as_deref(),
                    cache_dir: Some(&cache_dir),
                },
            )
            .await
        })))
    }

    /// Block on the verdict.
    pub(crate) async fn wait(mut self) -> Result<(), pnpm_lockfile_verification::VerifyError> {
        (&mut self.0).await.expect(
            "the lockfile verification task is only aborted by dropping the gate unawaited",
        )
    }
}

impl Drop for LockfileVerificationGate {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The Node version installability checks assume without probing: an
/// explicit `nodeVersion` config value first, then a
/// `devEngines.runtime` / `engines.runtime` pin from the root
/// manifest. The early host detection and the install paths must
/// derive it identically or the pre-spawned host would disagree with
/// the one the install would have detected.
fn effective_node_version(config: &Config, manifest: &PackageManifest) -> Option<String> {
    config.node_version.clone().or_else(|| node_version_from_engines_runtime(manifest.value()))
}

/// Shared out-map for [`ResolutionInputs::peer_issues_sink`]: importer id →
/// that importer's peer-dependency issues from the fresh resolve.
pub type PeerIssuesSink = Arc<
    std::sync::Mutex<
        std::collections::BTreeMap<String, pnpm_resolving_deps_resolver::PeerDependencyIssues>,
    >,
>;

/// Shared out-slot for [`ResolutionInputs::deps_requiring_build_sink`]: the dep
/// paths of every package this install put on disk whose files carry
/// install scripts (`requiresBuild`), regardless of the allow-build
/// policy. A snapshot skipped for installability, an excluded optional,
/// or a failed optional fetch is not installed and so not reported.
///
/// Only a fresh resolve that materializes `node_modules` fills the slot.
/// The frozen path and `lockfileOnly` runs leave it `None`, mirroring the
/// TypeScript CLI's `returnListOfDepsRequiringBuild`, which computes the
/// list from a fresh resolve's fetch results.
pub type DepsRequiringBuildSink = Arc<std::sync::Mutex<Option<BTreeSet<String>>>>;

pub struct WorkspaceInstallSelection<'a> {
    pub all_projects: &'a [pnpm_workspace::Project],
    pub project_dependencies: &'a indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    pub ordered_dirs: &'a [PathBuf],
    /// Projects chosen by the original filter. Manifest mutations stay
    /// scoped to these projects.
    pub selected_dirs: &'a HashSet<PathBuf>,
    /// The subset of [`Self::selected_dirs`] whose manifests the command
    /// changed; `None` when it changed every one of them.
    pub edited_dirs: Option<&'a HashSet<PathBuf>>,
    /// Importers to materialize: [`Self::selected_dirs`] plus an omitted
    /// workspace root that pnpm treats as a full-install importer.
    pub install_dirs: &'a HashSet<PathBuf>,
    pub active_manifest_is_standin: bool,
    pub workspace_cycles: PrecomputedWorkspaceCycles<'a>,
}

/// Whether the caller of a selected install already looked for
/// dependency cycles among the selected projects.
///
/// A caller may pass [`Self::Known`] only when it ran
/// [`fn@crate::workspace_cycles`] over the very graph the install would
/// rebuild — the same projects, in the same order, with the same graph
/// options — so the report (its cycle order included) stays what the
/// install's own [`crate::install_scope_cycles`] would emit.
#[derive(Debug, Default, Clone, Copy)]
pub enum PrecomputedWorkspaceCycles<'a> {
    /// It did not; the install runs its own cycle search.
    #[default]
    Unknown,
    /// The cycle report for this selection; `None` — the projects are
    /// orderable.
    Known(Option<&'a [Vec<PathBuf>]>),
}

/// What this run does to the manifests of the projects it installs —
/// pnpm's `MutatedProject.mutation`, which decides both whether the run
/// counts as a full install and which projects fire their own
/// `preinstall`/`install`/`postinstall`/`prepare` scripts.
///
/// pnpm builds a *mutated importer* list per command: the projects the
/// command acts on, plus the workspace root, which its recursive dispatch
/// pushes in as a plain `mutation: 'install'` whenever the selection
/// leaves it out. A project runs its own scripts when that list covers
/// only part of the workspace, or — when it covers all of it — when the
/// project's own mutation is a full install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMutation {
    /// pnpm's workspace-wide `mutation: 'install'`: `pacquet install`,
    /// `dedupe`, `prune`. Every project the run materializes is
    /// installed in full and runs its own scripts.
    InstallWorkspace,
    /// `pacquet deploy`. Deploys the project without running `prepare` scripts.
    Deploy,
    /// pnpm's `mutation: 'install'` narrowed to the projects the command
    /// was pointed at: a selector-less `pacquet update`, which installs
    /// those projects in full but leaves the rest of the workspace alone.
    InstallSelected,
    /// pnpm's `mutation: 'installSome'`: `pacquet add`,
    /// `pacquet update <selector>` and `pacquet update --latest`, which
    /// rewrite named dependencies rather than installing the project's
    /// whole manifest.
    InstallSome,
    /// pnpm's `mutation: 'uninstallSome'`: `pacquet remove`, which
    /// deletes named dependencies from the manifest before the install
    /// runs. The edited projects run the uninstall stages, not the
    /// install stages.
    UninstallSome,
    /// A run that installs no project's manifest: the commands that
    /// only materialize what the lockfile already records
    /// (`link`, `import`, `fetch`, `rebuild`).
    NoInstall,
}

impl ProjectMutation {
    /// Whether this run is a full project install (pnpm's
    /// `mutation: 'install'`) rather than a partial one.
    #[must_use]
    pub fn is_full_install(self) -> bool {
        matches!(
            self,
            ProjectMutation::InstallWorkspace
                | ProjectMutation::InstallSelected
                | ProjectMutation::Deploy,
        )
    }

    /// Whether the run may absorb its manifest drift by rewriting the
    /// loaded lockfile instead of resolving. `pacquet remove` qualifies
    /// because its only drift is the importer edges it deleted, and
    /// `pacquet add` because it pins the manifest before the install runs,
    /// leaving the same importer-edge drift — one the rewrite absorbs only
    /// when the lockfile already holds a version satisfying it.
    #[must_use]
    pub fn may_fast_update_lockfile(self) -> bool {
        self.is_full_install()
            || matches!(self, ProjectMutation::UninstallSome | ProjectMutation::InstallSome)
    }
}

pub(crate) fn selected_project_indices(
    projects: &[pnpm_workspace::Project],
    ordered_dirs: &[PathBuf],
    selected_dirs: &HashSet<PathBuf>,
) -> Vec<usize> {
    let project_indices = projects
        .iter()
        .enumerate()
        .map(|(index, project)| (project.root_dir.as_path(), index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen_dirs = HashSet::with_capacity(selected_dirs.len());
    let indices = ordered_dirs
        .iter()
        .filter(|dir| selected_dirs.contains(*dir))
        .map(|dir| {
            assert!(seen_dirs.insert(dir.as_path()), "selected project must be ordered once");
            *project_indices.get(dir.as_path()).expect("every selected project must be discovered")
        })
        .collect::<Vec<_>>();
    assert_eq!(seen_dirs.len(), selected_dirs.len(), "every selected project must be ordered");
    indices
}

/// This subroutine does everything `pacquet install` is supposed to do.
#[must_use]
pub struct Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub lockfile_policy: InstallLockfilePolicy,
    pub execution: InstallExecution,
    pub resolution: ResolutionInputs,
    pub context: InstallInvocation<'a>,
    pub fetching: InstallFetching<'a>,
    pub projects: InstallProjects<DependencyGroupList>,
}

#[derive(Clone, Copy)]
pub struct InstallInvocation<'a> {
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub manifest: &'a PackageManifest,
    /// Emit `pnpm:package-manifest initial` from this install run.
    /// Partial mutations that need the pre-mutation manifest snapshot
    /// emit it before changing the manifest and pass `false` here.
    pub emit_initial_manifest: bool,
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Absolute path of the loaded `pnpm-lock.yaml`. Threaded into
    /// the lockfile-verification gate so the per-path stat shortcut
    /// in `<cache_dir>/lockfile-verified.jsonl` can fire on repeat
    /// installs, and into the `pnpm:lockfile-verification` reporter
    /// payload. `None` disables the cache for this run (every call
    /// re-verifies) and falls back to deriving the path from
    /// `workspace_root`.
    pub lockfile_path: Option<&'a Path>,
}

pub struct InstallFetching<'a> {
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// prefetcher constructed in [`InstallWithFreshLockfile::run`]
    /// can capture an owned clone into the background download task
    /// while the install-side calls still take `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    /// Same client behind an [`Arc`] for the lockfile-verification
    /// gate (which owns its `ThrottledClient` to outlive the
    /// per-call lifetime of [`InstallInvocation::http_client`]). The CLI builds
    /// both from a single source; the duplicate is the smallest
    /// change that bridges the borrowed `&` shape every existing
    /// sub-installer expects with the owned `Arc` the verifier
    /// needs.
    pub http_client_arc: Arc<ThrottledClient>,
}

pub struct InstallProjects<DependencyGroupList> {
    pub dependency_groups: DependencyGroupList,
    /// `supportedArchitectures` after merging
    /// `Config::supported_architectures` from `pnpm-workspace.yaml`
    /// with the CLI per-axis overrides (`--cpu` / `--os` / `--libc`).
    /// Threaded into `InstallabilityHost` in the frozen-lockfile
    /// path so optional platform-tagged dependencies for the listed
    /// triples are kept even when they don't match the host. `None`
    /// means "host triple is the sole accept set" — the behavior
    /// when neither yaml nor CLI sets a value.
    ///
    /// Computed at the CLI layer (see
    /// `pnpm_cli::cli_args::supported_architectures::SupportedArchitecturesArgs`)
    /// instead of being read from `config` directly, because
    /// `State.config` is a shared `&'static Config` — the CLI
    /// override merge happens in the caller and lands here as a
    /// fully-resolved value.
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    /// In-memory catalogs to resolve against instead of reading
    /// `pnpm-workspace.yaml` from disk. `None` (every plain install) reads
    /// the workspace manifest. `pacquet update` sets this so a `--latest`
    /// catalog bump drives resolution even under `--no-save`, where the
    /// bumped entry is intentionally not persisted to disk.
    pub catalogs_override: Option<Catalogs>,
    /// In-process `readPackage` / `afterAllResolved` hooks supplied by an
    /// embedder (the Node API binding) instead of a `.pnpmfile.cjs` on disk.
    /// `Some` replaces the disk lookup for the install, including custom
    /// fetchers on the frozen path. `None` loads the configured pnpmfiles.
    pub pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    /// Workspace importers supplied in memory by an embedder (the Node API
    /// binding) instead of discovering them from a `pnpm-workspace.yaml` on
    /// disk. `Some` bypasses the on-disk workspace-project walk entirely — the
    /// root importer still comes from [`InstallInvocation::manifest`], siblings from this
    /// list. `None` (every CLI install) walks the workspace on disk.
    pub workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
}

struct InstallRunOptions<'install, 'selection> {
    lockfile_verification_override: Option<LockfileVerificationOverride<'install>>,
    rebuild: Option<RebuildOptions>,
    selection: Option<WorkspaceInstallSelection<'selection>>,
    root_manifest_as_workspace_root: bool,
    /// pnpm's `saveLockfile`: whether the resolved graph may be written
    /// to `<workspace_root>/pnpm-lock.yaml`. `false` for an install
    /// whose resolution belongs to a project other than the one that
    /// owns that lockfile, so the run must leave it untouched.
    save_lockfile: bool,
    /// pnpm's `lockfileCheck`: the caller restores the lockfile and diffs
    /// it once the install returns, so the run must leave nothing else on
    /// disk changed either. Only `pacquet dedupe --check` sets it.
    lockfile_check: bool,
    /// Forces the interactive-prompt eligibility that is otherwise derived
    /// from the process environment, so tests can exercise both branches.
    prompt_eligibility_override: Option<bool>,
    manifests: InstallManifestOptions<'install>,
}

#[derive(Default)]
struct InstallManifestOptions<'install> {
    deploy_hook: bool,
    /// Project manifests used only as the source for lockfile importer
    /// specifiers. `pacquet update --no-save` resolves against an in-memory
    /// manifest rewrite but must serialize importer specifiers from the
    /// manifest the user kept on disk. Supplied already
    /// `readPackage`-transformed.
    specifier_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    /// Manifest paths `pacquet update --no-save` already ran `readPackage`
    /// over before preparing its in-memory resolution rewrite. The hook must
    /// observe each project manifest exactly once, so the install layer skips
    /// these and still hooks every project manifest outside the set — the
    /// workspace projects the non-selected update path never loads. Dependency
    /// manifests always flow through the resolver's hook path.
    hooked_paths: HashSet<PathBuf>,
    /// See [`crate::ManifestSpecBumps`]. Only `pacquet update` sets it.
    spec_bumps: Option<&'install crate::ManifestSpecBumps>,
}

impl Default for InstallRunOptions<'_, '_> {
    fn default() -> Self {
        InstallRunOptions {
            lockfile_verification_override: None,
            rebuild: None,
            selection: None,
            root_manifest_as_workspace_root: false,
            save_lockfile: true,
            lockfile_check: false,
            prompt_eligibility_override: None,
            manifests: crate::install::InstallManifestOptions {
                deploy_hook: false,
                specifier_manifests: None,
                hooked_paths: HashSet::new(),
                spec_bumps: None,
            },
        }
    }
}

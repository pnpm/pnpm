pub use entry_points::apply_deploy_manifest_hook;
pub(crate) use entry_points::apply_deploy_manifest_hook_to_arc;
pub use errors::{InstallError, defer_ignored_builds};
pub(crate) use lockfile_freshness::{
    CheckLockfileSettingsDriftOptions, FreshnessCheckError, FreshnessScope,
    check_importer_satisfies, check_lockfile_settings_drift, parse_config_overrides,
};
pub use lockfile_freshness::{
    WantedLockfileSatisfactionCheck, wanted_lockfile_satisfies_workspace,
};
pub use workspace_state::{
    UpToDateFastPathCheck, UpToDateWorkspace, build_workspace_packages_map,
    check_deps_status_before_run_at, install_already_up_to_date,
};
pub(crate) use workspace_state::{build_workspace_state, lockfile_root_dir};

mod entry_points;

mod errors;

use errors::{map_fresh_lockfile_error, map_frozen_lockfile_error};

use crate::{
    HoistedDependencies, InstallFrozenLockfile, InstallWithFreshLockfile,
    InstallWithFreshLockfileError, LockfileVerificationOverride, OptimisticRepeatInstallCheck,
    PolicyExcludes, RebuildOptions, ResolvedPackages, UpdateSeedPolicy, build_resolution_verifiers,
    check_optimistic_repeat_install, emit_initial_package_manifest, link_project_bins,
    optimistic_repeat_install::Decision as OptimisticRepeatInstallDecision,
    prune_merged_branch_lockfile::prune_merged_branch_lockfile,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, NodeLinker, PNPM_VERSION};
use pnpm_executor::{
    DEV_PREINSTALL_ALREADY_RAN_ENV, RunPostinstallHooks,
    ScriptsPrependNodePath as ExecScriptsPrependNodePath, run_dev_preinstall_hook,
    run_project_lifecycle_scripts,
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
    io::IsTerminal,
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
    run_dev_preinstall, run_projects_lifecycle_scripts,
};
use lockfile_freshness::{
    FastUpdateLockfileOptions, check_lockfile_freshness, try_fast_update_lockfile,
};
use materialize::{MaterializationInputs, Materialized, materialize};
use modules_state::{
    build_modules_manifest, check_modules_settings_diff, current_contains_dep_path,
    drain_settled_projects, frozen_tree_intact, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, has_newly_allowed_ignored_builds,
    has_revoked_allowed_builds, manifest_string_field, merge_filtered_modules_metadata,
    merge_pending_builds, modules_consistent_with, modules_layout_consistent_with,
    project_requires_lifecycle_scripts, unapproved_recorded_ignored_builds,
};
use prepare_modules_state::{
    PrepareModulesStateInputs, PreparedModulesState, prepare_modules_state,
    prior_hoisted_dependencies, prior_hoisted_locations,
};
use workspace_state::{
    ProjectScriptsInputs, build_project_manifests_list, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, configured_or_discovered_workspace_dir,
    lockfile_root_for, projects_running_own_scripts, selected_manifest_freshness_inputs,
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
        (&mut self.0)
            .await
            .expect("the lockfile verification task is only aborted by dropping the gate unawaited")
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

/// Shared out-map for [`Install::peer_issues_sink`]: importer id →
/// that importer's peer-dependency issues from the fresh resolve.
pub type PeerIssuesSink = Arc<
    std::sync::Mutex<
        std::collections::BTreeMap<String, pnpm_resolving_deps_resolver::PeerDependencyIssues>,
    >,
>;

/// Shared out-slot for [`Install::deps_requiring_build_sink`]: the dep
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
    /// `dedupe`, `prune`, `deploy`. Every project the run materializes is
    /// installed in full and runs its own scripts.
    InstallWorkspace,
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
    /// runs.
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
        matches!(self, ProjectMutation::InstallWorkspace | ProjectMutation::InstallSelected)
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
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// prefetcher constructed in [`InstallWithFreshLockfile::run`]
    /// can capture an owned clone into the background download task
    /// while the install-side calls still take `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    /// Same client behind an [`Arc`] for the lockfile-verification
    /// gate (which owns its `ThrottledClient` to outlive the
    /// per-call lifetime of [`Self::http_client`]). The CLI builds
    /// both from a single source; the duplicate is the smallest
    /// change that bridges the borrowed `&` shape every existing
    /// sub-installer expects with the owned `Arc` the verifier
    /// needs.
    pub http_client_arc: Arc<ThrottledClient>,
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
    pub dependency_groups: DependencyGroupList,
    pub frozen_lockfile: bool,
    /// `preferFrozenLockfile` value to honor for *this* invocation.
    /// `None` (no CLI flag) means "use `config.prefer_frozen_lockfile`";
    /// `Some(true)` forces the auto-frozen fast path on, `Some(false)`
    /// forces it off. Computed at the CLI layer from the
    /// `--prefer-frozen-lockfile` / `--no-prefer-frozen-lockfile`
    /// flags. Threaded as an [`Option<bool>`] so the dispatch can
    /// tell a per-invocation override apart from the config default.
    pub prefer_frozen_lockfile: Option<bool>,
    /// Skip the per-importer `package.json` ↔ `pnpm-lock.yaml`
    /// freshness check ([`satisfies_package_manifest`]) that
    /// normally guards `--frozen-lockfile`. Surfaced as
    /// `--ignore-manifest-check` on the CLI; intended for the
    /// `configDependencies` delegation path, where the lockfile has
    /// just been resolved and written but the updated manifest hasn't
    /// been written yet. Settings-drift checks (`overrides`,
    /// `ignoredOptionalDependencies`, ...) still run — they don't
    /// inspect the manifest and the bug this flag addresses is
    /// specifically the per-dep specifier mismatch.
    pub ignore_manifest_check: bool,
    /// When `true`, runtime dependencies (`node@runtime:` /
    /// `deno@runtime:` / `bun@runtime:`) are skipped — their
    /// archives aren't fetched, their slots aren't materialized,
    /// and their bins aren't linked. Computed at the CLI layer
    /// from `config.skip_runtimes || --no-runtime`. The rest of
    /// the install proceeds normally. See
    /// `pnpm_config::Config::skip_runtimes`.
    pub skip_runtimes: bool,
    /// Effective `trustLockfile` value for *this* invocation. The CLI
    /// layer ORs the `--trust-lockfile` flag with `config.trust_lockfile`
    /// so a yaml `true` can't be overridden back to `false` from the
    /// CLI — the same stance applied to similar flags. Threaded as a
    /// separate field for the same reason [`Self::skip_runtimes`] is:
    /// `state.config` is a shared `&'static Config`, so the CLI
    /// override merge happens in the caller and lands here as a
    /// fully-resolved value.
    pub trust_lockfile: bool,
    /// The `--update-checksums` flag: refresh locked integrity values
    /// from the registry. Skips the frozen-lockfile path so the
    /// fresh-resolve path rewrites them.
    pub update_checksums: bool,
    /// What this run does to the manifests of the projects it installs.
    /// Decides whether the run counts as a full install and which
    /// projects fire their own lifecycle scripts — see
    /// [`ProjectMutation`].
    pub mutation: ProjectMutation,
    /// Whether every mutation this run performs is a plain install
    /// (upstream's `installsOnly`, true for `pacquet install` /
    /// `pacquet update`). A plain install may recreate a modules
    /// directory whose layout settings drifted; `add` / `remove` set
    /// this `false` and fail with the upstream `*_DIFF` errors
    /// instead — pnpm's `validateModules` contract. Distinct from
    /// [`ProjectMutation::is_full_install`], which stays `false` for a
    /// named `update`.
    pub installs_only: bool,
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
    /// `nodeLinker` value to honor for *this* invocation. The CLI
    /// layer applies any `--node-linker` override here; absent a
    /// flag, this equals `config.node_linker`. Threaded as a
    /// separate field for the same reason
    /// [`Self::supported_architectures`] is: `state.config` is a
    /// shared `&'static Config`, so the CLI override merge happens
    /// in the caller and lands here as a fully-resolved value.
    /// Used today for the `.modules.yaml.nodeLinker` write and
    /// (in Slice 6) for the install-pipeline branch.
    pub node_linker: pnpm_config::NodeLinker,
    /// When `true`, resolve dependencies and (re)write `pnpm-lock.yaml`
    /// but skip every materialization step: no tarball is fetched into
    /// the store, no `node_modules` is linked, and neither
    /// `.modules.yaml` nor the current lockfile
    /// (`<virtual_store_dir>/lock.yaml`) nor the workspace-state file
    /// is written. Surfaced as `--lockfile-only` on the CLI. A pure
    /// per-invocation flag (no `pnpm-workspace.yaml` / `config.yaml`
    /// counterpart — `lockfile-only` is an excluded config key),
    /// so it is threaded straight from the CLI like
    /// [`Self::frozen_lockfile`]. Equivalent to npm's
    /// `--package-lock-only`.
    pub lockfile_only: bool,
    /// `--dry-run`: resolve fully but write nothing, then report what a
    /// real install would change. Forces the fresh-resolve path (so the
    /// would-be lockfile is always computed), suppresses every write —
    /// `pnpm-lock.yaml`, `node_modules`, `.modules.yaml`, the current
    /// lockfile, the workspace-state file — and exits 0 regardless of
    /// whether changes were found.
    pub dry_run: bool,
    /// What this run may do with the resolution-policy bypasses a pick
    /// needs, such as `minimumReleaseAge` picks appended to
    /// `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`. Ignored on the
    /// frozen path, which resolves nothing. [`PolicyExcludes::Persist`]
    /// also gates the prune of that manifest's exclude lists
    /// (`minimumReleaseAgeExcludePrune` / `trustPolicyExcludePrune`) that
    /// this run owns; `add`, `update` and `remove` prune for themselves.
    pub policy_excludes: PolicyExcludes,
    /// Which lockfile pins to withhold from the preferred-versions seed.
    /// [`UpdateSeedPolicy::KeepAll`] for `install` / `add`; the `DropAll`
    /// / `DropOnly` variants drive `pacquet update`'s compatible bump by
    /// forcing the affected names to re-resolve to highest-in-range.
    /// Forwarded to [`InstallWithFreshLockfile`]; ignored on the frozen
    /// path (`update` always takes the fresh-resolve path). When set to
    /// anything other than `KeepAll` the optimistic repeat-install
    /// short-circuit is also bypassed so an `update` that finds newer
    /// in-range versions isn't skipped as "already up to date".
    pub update_seed_policy: UpdateSeedPolicy,
    /// Preferences layered onto the preferred-versions seed, by package
    /// name. `add` / `update` put a version named on the command line here
    /// so the re-resolve lands on it rather than on the highest version its
    /// range allows. Forwarded to [`InstallWithFreshLockfile`].
    pub preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    /// Per-invocation `Authorization`-header override for resolve/verify;
    /// `None` (every local install) uses `config.auth_headers`. The pnpr
    /// resolver threads request-scoped [`AuthHeaders`] here so it
    /// resolves a caller's private content without baking per-user auth
    /// into the shared `&'static Config`.
    pub auth_override: Option<Arc<AuthHeaders>>,
    /// Sink notified for each resolved tarball package as the fresh
    /// resolve yields it. `None` for every local install. The pnpr
    /// server installs one to stream fetch frames to the client so
    /// tarball downloads overlap server-side resolution.
    /// Ignored on the frozen path (no tree walk to observe).
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    /// Out-channel for the fresh resolve's per-importer peer-dependency
    /// issues. `None` for every CLI install (issues are only logged).
    /// The napi `getPeerDependencyIssues` runs a `dry_run` install with
    /// a sink to collect them — and a sink-driven dry run suppresses
    /// the CLI's stdout diff report, since it is a programmatic query
    /// rather than an `--dry-run` preview. Only the fresh path fills
    /// it (the frozen path resolves nothing).
    pub peer_issues_sink: Option<crate::PeerIssuesSink>,
    /// Out-slot for the dep paths of packages requiring a build. `None`
    /// for every CLI install; the napi `install` sets one when the
    /// embedder asks for `returnListOfDepsRequiringBuild`. See
    /// [`crate::DepsRequiringBuildSink`] for when it is filled.
    pub deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    /// In-memory catalogs to resolve against instead of reading
    /// `pnpm-workspace.yaml` from disk. `None` (every plain install) reads
    /// the workspace manifest. `pacquet update` sets this so a `--latest`
    /// catalog bump drives resolution even under `--no-save`, where the
    /// bumped entry is intentionally not persisted to disk.
    pub catalogs_override: Option<Catalogs>,
    /// When `true`, repeat-install fast paths are disabled so the full
    /// install pipeline always runs. `pacquet prune` sets this because
    /// a fast path can short-circuit before the virtual-store sweep,
    /// meaning extraneous packages can survive a prune when the lockfile
    /// hasn't changed.
    pub disable_optimistic_repeat_install: bool,
    /// In-process `readPackage` / `afterAllResolved` hooks supplied by an
    /// embedder (the Node API binding) instead of a `.pnpmfile.cjs` on disk.
    /// `Some` replaces the disk lookup for the install, including custom
    /// fetchers on the frozen path. `None` loads the configured pnpmfiles.
    pub pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    /// Workspace importers supplied in memory by an embedder (the Node API
    /// binding) instead of discovering them from a `pnpm-workspace.yaml` on
    /// disk. `Some` bypasses the on-disk workspace-project walk entirely — the
    /// root importer still comes from [`Self::manifest`], siblings from this
    /// list. `None` (every CLI install) walks the workspace on disk.
    pub workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
}

struct InstallRunOptions<'install, 'selection> {
    lockfile_verification_override: Option<LockfileVerificationOverride<'install>>,
    rebuild: Option<RebuildOptions>,
    selection: Option<WorkspaceInstallSelection<'selection>>,
    root_manifest_as_workspace_root: bool,
    deploy_manifest_hook: bool,
    /// Project manifests used only as the source for lockfile importer
    /// specifiers. `pacquet update --no-save` resolves against an in-memory
    /// manifest rewrite but must serialize importer specifiers from the
    /// manifest the user kept on disk. Supplied already
    /// `readPackage`-transformed.
    lockfile_specifier_project_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    /// Manifest paths `pacquet update --no-save` already ran `readPackage`
    /// over before preparing its in-memory resolution rewrite. The hook must
    /// observe each project manifest exactly once, so the install layer skips
    /// these and still hooks every project manifest outside the set — the
    /// workspace projects the non-selected update path never loads. Dependency
    /// manifests always flow through the resolver's hook path.
    read_package_hooked_manifest_paths: HashSet<PathBuf>,
    /// pnpm's `saveLockfile`: whether the resolved graph may be written
    /// to `<workspace_root>/pnpm-lock.yaml`. `false` for an install
    /// whose resolution belongs to a project other than the one that
    /// owns that lockfile, so the run must leave it untouched.
    save_lockfile: bool,
    /// pnpm's `lockfileCheck`: the caller restores the lockfile and diffs
    /// it once the install returns, so the run must leave nothing else on
    /// disk changed either. Only `pacquet dedupe --check` sets it.
    lockfile_check: bool,
    /// See [`crate::ManifestSpecBumps`]. Only `pacquet update` sets it.
    manifest_spec_bumps: Option<&'install crate::ManifestSpecBumps>,
    /// Forces the interactive-prompt eligibility that is otherwise derived
    /// from the process environment, so tests can exercise both branches.
    prompt_eligibility_override: Option<bool>,
}

impl Default for InstallRunOptions<'_, '_> {
    fn default() -> Self {
        InstallRunOptions {
            lockfile_verification_override: None,
            rebuild: None,
            selection: None,
            root_manifest_as_workspace_root: false,
            deploy_manifest_hook: false,
            lockfile_specifier_project_manifests: None,
            read_package_hooked_manifest_paths: HashSet::new(),
            save_lockfile: true,
            lockfile_check: false,
            manifest_spec_bumps: None,
            prompt_eligibility_override: None,
        }
    }
}

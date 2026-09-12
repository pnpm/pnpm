pub use errors::InstallWithFreshLockfileError;
pub(crate) use lockfile_build::compute_package_extensions_checksum;
pub(crate) use seed_policy::prefer_requested_version;
pub use seed_policy::{ImporterUpdateSeedPolicy, UpdateSeedPolicy};

mod persist;
use persist::{
    LockfileOnlyOptions, finish_lockfile_only, fix_lockfile_copy, importers_consuming_linked_peers,
    persist_fresh_lockfile, verify_repair_if_filtered,
};

mod lockfile_build;

use lockfile_build::{
    build_lockfile_phase, compose_manifest_hooks, ignored_optional_dependencies_match,
    overrides_match, parse_config_overrides, resolved_overrides_map,
};

mod on_disk;
use on_disk::{OnDiskInputs, OnDiskOutput, finish_early_materialization, run_on_disk_phases};

mod plan;
use plan::{
    FinalScope, FreshPlan, HostProbeInputs, LockfileViews, MaterializationScope, PlanLockfiles,
    PlanScope, include_transitive_optional_dependencies, is_partial_workspace_selection,
    plan_fresh_materialization,
};

mod resolution;
use resolution::{ManifestSlots, Resolved, resolve_graph, start_early_materialization};

mod setup;
use setup::{InstallShape, ResolverSetup, set_up_resolvers};

mod materialization;
use materialization::finish_resolved_install;

mod errors;

mod seed_policy;

use crate::{HoistedDependencies, PolicyExcludes, SkippedSnapshots};
use dashmap::DashMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{
    DeprecationLog, GlobalLog, HookLog, LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalParent, SkippedOptionalReason,
};
use pnpm_resolving_npm_resolver::InMemoryPackageMetaCache;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_tarball::MemCache;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};
use tokio::sync::watch;

mod manifest_transforms;
mod resolve;
mod resolver_setup;

/// In-memory dedup gate for packages materialized during this install.
/// Keyed by virtual-store name (`{name-with-slashes-replaced}@{version}`).
///
/// The value is a [`watch::Sender<bool>`] whose state transitions from
/// `false` (slot reserved, first writer running) to `true` (the first
/// writer's materialization is complete, `save_path` is on disk).
/// Second visitors subscribe to the sender before issuing their
/// per-parent symlink so they don't race ahead of the first writer's
/// `import_indexed_dir` — critical on Windows where `symlink_package`
/// may fall back to a junction, which requires the target directory
/// to exist at creation time. Mirrors the implicit "wait until the
/// shared slot is on disk" sequencing pnpm gets from running one
/// resolveDependencyTree pass before the install pass.
pub type ResolvedPackages = DashMap<String, watch::Sender<bool>>;

/// Fresh-install path: resolve the project from the registry, fetch +
/// materialize `node_modules`, and emit a brand-new `pnpm-lock.yaml`
/// reflecting the resolved graph. Caller (see [`crate::Install::run`])
/// drives this path whenever no `--frozen-lockfile` was requested.
///
/// **Brief overview for each package:**
/// * Resolve every importer's dependency through the [`NpmResolver`][pnpm_resolving_npm_resolver::NpmResolver] chain
///   (`resolve_workspace` builds the per-importer trees, runs the
///   cross-importer peer pass, and applies `dedupeInjectedDeps`).
/// * Fetch a tarball of each resolved package and extract it into the
///   store directory.
/// * Import (by reflink, hardlink, or copy) the files from the store
///   dir to `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in
///   `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at `node_modules/{name}`.
/// * Run the resolved graph through
///   [`crate::dependencies_graph_to_lockfile()`] to produce a v9
///   `pnpm-lock.yaml`; the caller writes it to `<lockfile_dir>/pnpm-lock.yaml`.
#[must_use]
pub struct InstallWithFreshLockfile<'a> {
    pub(crate) inputs: FreshInputs<'a>,
    pub(crate) owned: OwnedInputs,
    /// One entry per importer to resolve, keyed by the lockfile
    /// importer id (`"."` for the workspace root, POSIX-relative path
    /// for sibling projects — see
    /// [`pnpm_workspace::importer_id_from_root_dir`]). For a
    /// non-workspace install this carries a single `"."` entry
    /// pointing at the only project.
    pub(crate) importer_manifests: BTreeMap<String, &'a PackageManifest>,
}

/// The install's borrowed inputs, as one `Copy` value the phases read.
#[derive(Clone, Copy)]
pub(crate) struct FreshInputs<'a> {
    pub(crate) http_client: &'a ThrottledClient,
    pub(crate) config: &'static Config,
    pub(crate) dependency_groups: &'a [DependencyGroup],
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub(crate) logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter `requester` fields.
    pub(crate) requester: &'a str,
    /// Lockfile root for the install, used by the resolver chain to
    /// compute `link:` / `file:` relative paths and to anchor
    /// workspace-package resolution. Equal to the manifest's
    /// parent directory under single-project installs and to the
    /// `pnpm-workspace.yaml` root under monorepos.
    pub(crate) lockfile_dir: &'a Path,
    /// Refresh locked integrity values from the registry. Threaded
    /// into [`ResolveOptions::update_checksums`][pnpm_resolving_resolver_base::ResolveOptions::update_checksums] so the picker bypasses
    /// its in-memory and on-disk metadata caches and always goes to
    /// the registry with conditional headers.
    pub(crate) update_checksums: bool,
    /// Existing `pnpm-lock.yaml` to seed `getPreferredVersionsFromLockfileAndManifests`
    /// with already-pinned `(name, version)` pairs. `Some` on the
    /// stale-lockfile / `preferFrozenLockfile: false` rewrite path
    /// — the resolver biases toward the seeded versions when they
    /// still satisfy the spec so unrelated dependencies keep their
    /// pins. `None` on the no-lockfile path. Corresponds to the
    /// `update: false` resolver mode.
    pub(crate) wanted_lockfile: Option<&'a Lockfile>,
    /// Intact prior lockfile used to restore unselected projects after a
    /// filtered repair resolves against a sanitized seed.
    pub(crate) merge_wanted_lockfile: Option<&'a Lockfile>,
    /// Resolved [`pnpm_config::Config::node_linker`]. Selects the
    /// materialization shape after the virtual store is populated:
    /// under [`NodeLinker::Hoisted`] the freshly-built lockfile is
    /// routed through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// symlink layout.
    pub(crate) node_linker: NodeLinker,
    /// CLI-merged `supportedArchitectures` (`pnpm-workspace.yaml` +
    /// `--cpu`/`--os`/`--libc`). Threaded into the hoisted-linker
    /// walker so its installability filter honors user-supplied
    /// accept lists. `None` when no architectures are configured.
    pub(crate) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// When `true`, resolve the graph and write `pnpm-lock.yaml`, then
    /// return — skipping the tarball prefetch, virtual-store
    /// materialization, symlinks, hoisting, and bin linking. The store
    /// stays untouched (no tarball is fetched) — a dry-run resolve pass.
    /// See [`crate::Install::lockfile_only`].
    pub(crate) lockfile_only: bool,
    /// `config.skip_runtimes || --no-runtime`; see
    /// [`crate::add_direct_runtime_skips`].
    pub(crate) skip_runtimes: bool,
    /// `--dry-run`: build the would-be lockfile but do not write it to
    /// disk. Implies [`Self::lockfile_only`] (nothing is materialized);
    /// the caller diffs the returned [`InstallWithFreshLockfileResult::wanted_lockfile`]
    /// against the existing one and reports the changes.
    pub(crate) dry_run: bool,
    /// Whether this invocation can safely read an interactive approval from
    /// stdin. Computed once by the outer install runner from CI and terminal
    /// state, with an explicit override available to deterministic tests.
    pub(crate) can_prompt: bool,
    /// What the run may do with resolution-policy bypasses; see
    /// [`crate::Install::policy_excludes`].
    pub(crate) policy_excludes: PolicyExcludes,
    /// A full workspace install versus a partial one (`pacquet add` and the
    /// package installs built on it — `dlx`, global add, the engine install).
    /// See [`crate::ProjectMutation::is_full_install`]. Gates the `--no-optional`
    /// exclusion: only a full install's `dependency_groups` carries that
    /// intent, so a partial run must not drop transitive optionals.
    pub(crate) is_full_install: bool,
    pub(crate) deploy_manifest_hook: bool,
    pub(crate) real_importer_ids: Option<&'a std::collections::HashSet<String>>,
    pub(crate) selected_importer_ids: Option<&'a std::collections::HashSet<String>>,
    /// What the previous install materialized
    /// (`<virtual_store_dir>/lock.yaml`). Drives the pre-link
    /// [`crate::PruneStaleModules`] reconciliation and the hoisted
    /// linker's previous-graph orphan diff. `None` on a first install.
    pub(crate) current_lockfile: Option<&'a Lockfile>,
    /// `hoistedDependencies` recorded by the previous install's
    /// `.modules.yaml`, for [`crate::PruneStaleModules`]'s orphan
    /// hoist-link cleanup. `None` on a first install or when the file
    /// couldn't be fully parsed.
    pub(crate) prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// `hoistedLocations` from the previous install for the already-in-place check.
    pub(crate) prior_hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// See [`crate::InstallFrozenLockfile::allow_builds_changed`].
    pub(crate) allow_builds_changed: bool,
    /// See [`crate::HoistedLinkerInputs::prior_unbuilt_builds`].
    pub(crate) prior_unbuilt_builds: &'a crate::UnbuiltBuilds,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub(crate) prune_orphans: bool,
    /// pnpm's `saveLockfile`: whether the freshly built lockfile may be
    /// written to `<lockfile_dir>/pnpm-lock.yaml`. `false` leaves that
    /// file untouched — the resolved graph is still returned and still
    /// drives `<virtual_store_dir>/lock.yaml`. See
    /// [`crate::Install::run_legacy_deploy`].
    pub(crate) save_lockfile: bool,
    /// The declared ranges `pacquet update` asks this run to move onto the
    /// versions it resolves, and the sink it reports them back through.
    /// `None` for every other install.
    pub(crate) manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    /// Resolution policies used to validate a filtered repair after the
    /// sanitized merge view has been spliced into the freshly resolved graph.
    pub(crate) resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
}

impl FreshInputs<'_> {
    fn included(&self) -> IncludedDependencies {
        IncludedDependencies {
            dependencies: self.dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: self.dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: self.dependency_groups.contains(&DependencyGroup::Optional),
        }
    }
}

/// The inputs the install consumes rather than borrows.
pub(crate) struct OwnedInputs {
    /// Which lockfile pins to withhold from the preferred-versions seed
    /// so the affected names re-resolve to the highest version
    /// satisfying their manifest range. Drives `pacquet update`'s
    /// compatible bump; see [`UpdateSeedPolicy`].
    pub(crate) update_seed_policy: UpdateSeedPolicy,
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// resolve-time prefetcher ([`PrefetchingResolver`][crate::PrefetchingResolver]) can capture
    /// an owned clone into the background download task spawned for
    /// each fresh resolution while the install-side per-package call
    /// in `install_subtree` still takes `&MemCache` via deref.
    pub(crate) tarball_mem_cache: Arc<MemCache>,
    /// Same client behind an [`Arc`] for the [`NpmResolver`][pnpm_resolving_npm_resolver::NpmResolver], whose
    /// stored `ThrottledClient` outlives any per-call borrow.
    pub(crate) http_client_arc: Arc<ThrottledClient>,
    /// Optional per-importer manifest source used only when serializing
    /// importer specifiers into the lockfile. `update --no-save` resolves
    /// against an in-memory manifest rewrite, while the lockfile importer
    /// entry must still reflect the kept on-disk manifest.
    pub(crate) lockfile_specifier_manifests: Option<BTreeMap<String, PackageManifest>>,
    /// Catalogs parsed from `pnpm-workspace.yaml`. Empty for projects
    /// without a workspace manifest.
    pub(crate) catalogs: Catalogs,
    /// Workspace-sibling lookup the [`NpmResolver`][pnpm_resolving_npm_resolver::NpmResolver] consults when it
    /// sees a `workspace:` spec. `None` when this install isn't inside
    /// a `pnpm-workspace.yaml` workspace; the resolver then errors out
    /// on any `workspace:` spec via
    /// `ResolveFromWorkspaceError::WorkspacePackagesNotLoaded` — the
    /// `Cannot resolve package from workspace because opts.workspacePackages is not defined`
    /// behavior.
    pub(crate) workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    /// An `Arc` handle to the same document as [`FreshInputs::wanted_lockfile`],
    /// when the loader holds one; `None` falls back to a deep copy where
    /// the resolver needs an owned handle.
    pub(crate) wanted_lockfile_shared: Option<Arc<Lockfile>>,
    /// Effective `nodeVersion`: an explicit config value, otherwise the
    /// minimum version declared by the root manifest's runtime engine.
    pub(crate) node_version: Option<String>,
    /// A host detection the install entry point spawned right after
    /// the wanted lockfile parsed (see
    /// [`pnpm_deps_restorer::materialization_plan::HostDetection::spawn`]).
    /// By the time resolution finishes its `node --version` has long
    /// completed, so the installability check that runs after
    /// resolution costs nothing. Must have been spawned with this
    /// install's `node_version` / `supported_architectures` /
    /// `engine_strict`. `None` runs the detection here.
    pub(crate) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    /// Per-install packument cache shared with the lockfile-verifier
    /// constructed in [`Install::run`](crate::Install::run). The
    /// resolver writes to it during `pick_package`; the verifier reads
    /// from it to skip duplicate fetches when both touch the same
    /// `(registry, name)`.
    pub(crate) meta_cache: Arc<InMemoryPackageMetaCache>,
    /// Preferences layered onto the seed, by package name. `add` / `update`
    /// put a version named on the command line here so the re-resolve lands
    /// on it instead of on the highest one its range allows.
    pub(crate) preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    /// Per-invocation `Authorization`-header override; `None` uses
    /// `config.auth_headers`. See [`crate::Install::auth_override`].
    pub(crate) auth_override: Option<Arc<AuthHeaders>>,
    /// Sink notified for each resolved tarball package as the tree walk
    /// yields it. `None` for every local install; the pnpr server sets
    /// one. See [`crate::Install::resolution_observer`].
    pub(crate) resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    /// Out-channel for the resolve's per-importer peer-dependency
    /// issues. See [`crate::Install::peer_issues_sink`].
    pub(crate) peer_issues_sink: Option<crate::PeerIssuesSink>,
    /// Out-slot for the dep paths of packages requiring a build. See
    /// [`crate::Install::deps_requiring_build_sink`].
    pub(crate) deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    /// In-process `readPackage`/`afterAllResolved` hooks supplied by an
    /// embedder instead of a `.pnpmfile.cjs` on disk. `Some` replaces the
    /// disk lookup entirely; `None` (every CLI install) falls back to
    /// [`load_pnpmfile`][pnpm_hooks::finder::load_pnpmfile]. See [`crate::Install::pnpmfile_hook_override`].
    pub(crate) pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    /// The pre-resolve verification of the existing lockfile, running in
    /// the background while this install resolves and materializes. The
    /// verdict is awaited before bin linking, dependency builds, and the
    /// lockfile save. See [`crate::LockfileVerificationGate`].
    pub(crate) lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
}

/// Output of [`InstallWithFreshLockfile::run`].
///
/// Returns the hoist-graph slot the dispatch already consumed plus the
/// freshly-built [`Lockfile`] (when the writer ran), so the caller can
/// save it as `<virtual_store_dir>/lock.yaml` after `.modules.yaml`
/// succeeds — the same ordering the frozen-lockfile path uses to
/// guarantee a manifest failure can't leave a current-lockfile
/// pointing at incomplete install state.
#[must_use]
pub struct InstallWithFreshLockfileResult {
    pub hoisted_dependencies: HoistedDependencies,
    /// Per-depPath list of lockfile-relative directory paths the
    /// hoisted linker placed each package at. Empty under the
    /// isolated linker (the field is hoisted-only on disk). The
    /// caller persists it into
    /// [`pnpm_modules_yaml::Modules::hoisted_locations`] so a
    /// follow-up install or rebuild can locate every package without
    /// re-running the walker.
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Per-source-project list of virtual-store package directories
    /// its injected `file:` copies were materialized at. Round-trips
    /// through [`pnpm_modules_yaml::Modules::injected_deps`] —
    /// see [`crate::collect_injected_deps`]. Empty on the
    /// `lockfile_only` path, which never materializes.
    pub injected_deps: BTreeMap<String, Vec<String>>,
    /// Importers the resolution left a peer-dependency issue under.
    /// Install completion renders its report from
    /// [`Self::wanted_lockfile`] — which carries the resolved versions
    /// the resolver's parent chains leave out — but walks only these
    /// importers.
    pub peer_issue_importer_ids: HashSet<String>,
    /// `Some` when the install resolved a graph that was written to
    /// `pnpm-lock.yaml`; `None` when the write was skipped (today: only
    /// `config.lockfile=false`). The caller mirrors the same gate when
    /// deciding whether to persist the current-lockfile.
    pub wanted_lockfile: Option<Lockfile>,
    /// `true` when the wanted lockfile written to disk is the same
    /// typed lockfile returned in [`Self::wanted_lockfile`]. A
    /// non-null `afterAllResolved` hook result can mutate fields the
    /// typed model tracks, so the caller must not record a verification
    /// cache entry for that case.
    pub can_record_lockfile_verification: bool,
    /// Sorted `name@version` keys whose build scripts were blocked by
    /// the `allowBuilds` policy. The caller raises
    /// `ERR_PNPM_IGNORED_BUILDS` from this list when `strictDepBuilds`
    /// is on (the default). Empty on the `lockfile_only` path, which
    /// never materializes or builds.
    pub ignored_builds: Vec<String>,
    /// Dep paths whose build `--ignore-scripts` deferred — see
    /// [`crate::BuildModulesOutput::deferred_builds`]. The caller folds
    /// them into `.modules.yaml.pendingBuilds`. Empty on the
    /// `lockfile_only` path for the same reason as
    /// [`Self::ignored_builds`].
    pub deferred_builds: Vec<String>,
    /// Installability-skipped optional snapshots. The outer install
    /// writer persists these into `.modules.yaml.skipped`.
    pub skipped: SkippedSnapshots,
    /// The store-index writer task, already winding down — see
    /// [`pnpm_deps_restorer::InstallFrozenLockfileOutput::store_index_teardown`]:
    /// every handle was dropped, the task is flushing its final batch
    /// and closing its `SQLite` connection (a WAL checkpoint). Await it
    /// via [`pnpm_store_dir::StoreIndexWriter::drain`] as late as
    /// possible so the close overlaps the caller's tail writes.
    pub store_index_teardown: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}

impl InstallWithFreshLockfile<'_> {
    /// Execute the subroutine.
    ///
    /// Under the isolated linker the [`HoistedDependencies`] result
    /// carries the publicly/privately-hoisted alias map; under
    /// `nodeLinker: hoisted` it is empty (the hoisted linker writes the
    /// on-disk tree directly and reports its placements through
    /// [`InstallWithFreshLockfileResult::hoisted_locations`] instead).
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
        let install = self.inputs;
        let mut owned = self.owned;
        let mut manifests = ManifestSlots::declared(self.importer_manifests);
        let mut setup = set_up_resolvers::<Reporter>(install, &mut owned).await?;
        let resolved =
            resolve_graph::<Reporter>(install, &mut owned, &mut setup, &mut manifests).await?;
        finish_resolved_install::<Reporter>(install, owned, setup, resolved).await
    }
}

/// Build the `context.log(...)` sink a pnpmfile hook forwards to: each
/// `context.log(message)` call emits a `pnpm:hook` event through the
/// install's reporter, carrying the project `prefix`, the pnpmfile path
/// (`from`), and the hook name.
pub(crate) fn hook_log_fn<Reporter: self::Reporter>(
    prefix: &Path,
    from: &Path,
    hook: &'static str,
) -> pnpm_hooks::LogFn {
    let prefix = prefix.to_string_lossy().into_owned();
    let from = from.to_string_lossy().into_owned();
    Arc::new(move |message: String| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level: LogLevel::Debug,
            from: from.clone(),
            hook: hook.to_string(),
            message,
            prefix: prefix.clone(),
        }));
    })
}

/// Build the resolver's skipped-optional-dependency sink: each
/// notification emits a `pnpm:skipped-optional-dependency` debug event
/// with `reason=resolution_failure` through the install's reporter,
/// matching pnpm's `skippedOptionalDependencyLogger.debug` payload.
fn skipped_optional_log_fn<Reporter: self::Reporter>()
-> pnpm_resolving_deps_resolver::SkippedOptionalLogFn {
    Arc::new(|skipped: pnpm_resolving_deps_resolver::SkippedOptionalDependency| {
        Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
            level: LogLevel::Debug,
            details: Some(skipped.details),
            package: SkippedOptionalPackage::ResolutionFailure {
                name: skipped.name,
                version: skipped.version,
                bare_specifier: skipped.bare_specifier,
            },
            parents: Some(
                skipped
                    .parents
                    .into_iter()
                    .map(|parent| SkippedOptionalParent {
                        id: parent.id,
                        name: parent.name,
                        version: parent.version,
                    })
                    .collect(),
            ),
            prefix: skipped.prefix,
            reason: SkippedOptionalReason::ResolutionFailure,
        }));
    })
}

/// Build the resolver's deprecation sink: each notification emits a
/// `pnpm:deprecation` debug event through the install's reporter,
/// matching pnpm's `deprecationLogger.debug` payload.
fn deprecation_log_fn<Reporter: self::Reporter>() -> pnpm_resolving_deps_resolver::DeprecationLogFn
{
    Arc::new(|deprecation: pnpm_resolving_deps_resolver::Deprecation| {
        Reporter::emit(&LogEvent::Deprecation(DeprecationLog {
            level: LogLevel::Debug,
            pkg_name: deprecation.pkg_name,
            pkg_version: deprecation.pkg_version,
            pkg_id: deprecation.pkg_id,
            prefix: deprecation.prefix,
            deprecated: deprecation.deprecated,
            depth: deprecation.depth,
        }));
    })
}

/// Build one side of the `preResolution` hook's `logger`: each
/// `logger.info(...)` / `logger.warn(...)` call emits a `pnpm:hook` event at
/// the given level. `from` is the literal `"pnpmfile"` — pnpm's
/// `createPreResolutionHookLogger` hardcodes it rather than passing the
/// pnpmfile path.
fn pre_resolution_log_fn<Reporter: self::Reporter>(
    prefix: &Path,
    level: LogLevel,
) -> pnpm_hooks::LogFn {
    let prefix = prefix.to_string_lossy().into_owned();
    Arc::new(move |message: String| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level,
            from: "pnpmfile".to_string(),
            hook: "preResolution".to_string(),
            message,
            prefix: prefix.clone(),
        }));
    })
}

/// The importer selection the patch-usage check is scoped by.
#[derive(Clone, Copy)]
struct PatchUsageScope<'a> {
    real_importer_ids: Option<&'a HashSet<String>>,
    selected_importer_ids: Option<&'a HashSet<String>>,
    merge_wanted_lockfile: Option<&'a Lockfile>,
}

/// Only in the fresh-lockfile path — a frozen lockfile trusts recorded
/// patches. pnpm's importer-count gate admits an unfiltered run, or a filtered
/// run whose root-augmented selection and previous wanted lockfile both cover
/// the complete workspace. A first filtered install has no complete previous
/// lockfile and skips this check.
fn check_patch_usage<Reporter: self::Reporter>(
    config: &Config,
    patched_dependencies: Option<&pnpm_patching::PatchGroupRecord>,
    applied_patches: &rustc_hash::FxHashSet<String>,
    scope: PatchUsageScope<'_>,
) -> Result<(), InstallWithFreshLockfileError> {
    let Some(deps) = patched_dependencies else { return Ok(()) };
    let verify = match scope.selected_importer_ids {
        None => true,
        Some(selected_importer_ids) => {
            !is_partial_workspace_selection(scope.real_importer_ids, Some(selected_importer_ids))
                && scope.merge_wanted_lockfile.is_some_and(|wanted_lockfile| {
                    wanted_lockfile.importers.len() == selected_importer_ids.len()
                })
        }
    };
    if !verify {
        return Ok(());
    }
    match pnpm_patching::verify_patches(
        deps,
        &applied_patches.iter().cloned().collect(),
        config.allow_unused_patches,
    ) {
        Ok(None) => Ok(()),
        Ok(Some(warning)) => {
            Reporter::emit(&LogEvent::Global(GlobalLog {
                level: LogLevel::Warn,
                message: warning.to_string(),
            }));
            Ok(())
        }
        Err(err) => Err(InstallWithFreshLockfileError::UnusedPatch(err)),
    }
}

/// Hand the per-importer peer issues to the programmatic caller before the
/// graph is consumed, and log them.
fn report_peer_issues(
    sink: Option<&crate::PeerIssuesSink>,
    issues_by_importer: &BTreeMap<String, pnpm_resolving_deps_resolver::PeerDependencyIssues>,
) {
    if let Some(sink) = sink {
        *sink.lock().expect("peer-issues sink lock poisoned") = issues_by_importer.clone();
    }
    for (importer_id, issues) in issues_by_importer {
        tracing::warn!(
            target: "pacquet::install",
            importer_id = %importer_id,
            missing = issues.missing.len(),
            bad = issues.bad.len(),
            "Peer dependency issues detected (issue renderer not ported yet)",
        );
    }
}

/// No-op unless the run both parsed overrides and built an overrider for
/// them; only then are the collected declared ranges complete enough for a
/// staleness verdict.
async fn warn_stale_convergence_overrides_if_any<Reporter: pnpm_reporter::Reporter>(
    npm_resolver: &dyn pnpm_resolving_resolver_base::Resolver,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    versions_overrider: Option<&crate::VersionsOverrider>,
    lockfile_dir: &Path,
    published_by: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<&pnpm_config::version_policy::PackageVersionPolicy>,
    ),
) {
    let (Some(parsed), Some(overrider)) = (parsed_overrides, versions_overrider) else { return };
    let (published_by, published_by_exclude) = published_by;
    resolve::warn_stale_convergence_overrides::<Reporter>(
        npm_resolver,
        parsed,
        overrider,
        lockfile_dir,
        published_by,
        published_by_exclude,
    )
    .await;
}

#[cfg(test)]
mod tests;

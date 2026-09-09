use crate::{
    AllowBuildPolicy, CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput,
    DependenciesGraphToLockfileError, GraphToLockfileOptions, HoistedDependencies,
    ImporterLockfileInput, InstallPackageFromRegistryError, LinkRootComponentMembersError,
    LinkVirtualStoreBinsError, SkippedSnapshots, SymlinkDirectDependenciesError,
    VersionPolicyError, VirtualStoreLayout, dependencies_graph_to_lockfile,
    store_init::init_store_dir_best_effort,
};
use dashmap::DashMap;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use pnpm_cmd_shim::LinkBinsError;
use pnpm_config::{Config, NodeLinker, TrustPolicy};
use pnpm_lockfile::{Lockfile, LockfileEntries, SaveLockfileError};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{
    DeprecationLog, GlobalLog, HookLog, LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalParent, SkippedOptionalReason, Stage, StageLog,
};
use pnpm_resolving_deps_resolver::{
    ManifestHook, ResolveDependencyTreeError, UpdateDepth, UpdateTargets,
};
use pnpm_resolving_npm_resolver::{InMemoryPackageMetaCache, MergeNamedRegistriesError};
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_tarball::MemCache;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// resolve-time prefetcher ([`PrefetchingResolver`][crate::PrefetchingResolver]) can capture
    /// an owned clone into the background download task spawned for
    /// each fresh resolution while the install-side per-package call
    /// in `install_subtree` still takes `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    /// Same client behind an [`Arc`] for the [`NpmResolver`][pnpm_resolving_npm_resolver::NpmResolver], whose
    /// stored `ThrottledClient` outlives any per-call borrow.
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    /// One entry per importer to resolve, keyed by the lockfile
    /// importer id (`"."` for the workspace root, POSIX-relative path
    /// for sibling projects — see
    /// [`pnpm_workspace::importer_id_from_root_dir`]). For a
    /// non-workspace install this carries a single `"."` entry
    /// pointing at the only project.
    pub importer_manifests: BTreeMap<String, &'a PackageManifest>,
    /// Optional per-importer manifest source used only when serializing
    /// importer specifiers into the lockfile. `update --no-save` resolves
    /// against an in-memory manifest rewrite, while the lockfile importer
    /// entry must still reflect the kept on-disk manifest.
    pub lockfile_specifier_manifests: Option<BTreeMap<String, PackageManifest>>,
    pub dependency_groups: &'a [DependencyGroup],
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter `requester` fields.
    pub requester: &'a str,
    /// Catalogs parsed from `pnpm-workspace.yaml`. Empty for projects
    /// without a workspace manifest.
    pub catalogs: Catalogs,
    /// Lockfile root for the install, used by the resolver chain to
    /// compute `link:` / `file:` relative paths and to anchor
    /// workspace-package resolution. Equal to the manifest's
    /// parent directory under single-project installs and to the
    /// `pnpm-workspace.yaml` root under monorepos.
    pub lockfile_dir: &'a Path,
    /// Workspace-sibling lookup the [`NpmResolver`][pnpm_resolving_npm_resolver::NpmResolver] consults when it
    /// sees a `workspace:` spec. `None` when this install isn't inside
    /// a `pnpm-workspace.yaml` workspace; the resolver then errors out
    /// on any `workspace:` spec via
    /// `ResolveFromWorkspaceError::WorkspacePackagesNotLoaded` — the
    /// `Cannot resolve package from workspace because opts.workspacePackages is not defined`
    /// behavior.
    pub workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    /// Refresh locked integrity values from the registry. Threaded
    /// into [`ResolveOptions::update_checksums`][pnpm_resolving_resolver_base::ResolveOptions::update_checksums] so the picker bypasses
    /// its in-memory and on-disk metadata caches and always goes to
    /// the registry with conditional headers.
    pub update_checksums: bool,
    /// Existing `pnpm-lock.yaml` to seed `getPreferredVersionsFromLockfileAndManifests`
    /// with already-pinned `(name, version)` pairs. `Some` on the
    /// stale-lockfile / `preferFrozenLockfile: false` rewrite path
    /// — the resolver biases toward the seeded versions when they
    /// still satisfy the spec so unrelated dependencies keep their
    /// pins. `None` on the no-lockfile path. Corresponds to the
    /// `update: false` resolver mode.
    pub wanted_lockfile: Option<&'a Lockfile>,
    /// An `Arc` handle to the same document as [`Self::wanted_lockfile`],
    /// when the loader holds one; `None` falls back to a deep copy where
    /// the resolver needs an owned handle.
    pub wanted_lockfile_shared: Option<Arc<Lockfile>>,
    /// Intact prior lockfile used to restore unselected projects after a
    /// filtered repair resolves against a sanitized seed.
    pub merge_wanted_lockfile: Option<&'a Lockfile>,
    /// Effective `nodeVersion`: an explicit config value, otherwise the
    /// minimum version declared by the root manifest's runtime engine.
    pub node_version: Option<String>,
    /// A host detection the install entry point spawned right after
    /// the wanted lockfile parsed (see
    /// [`pnpm_deps_restorer::materialization_plan::HostDetection::spawn`]).
    /// By the time resolution finishes its `node --version` has long
    /// completed, so the installability check that runs after
    /// resolution costs nothing. Must have been spawned with this
    /// install's `node_version` / `supported_architectures` /
    /// `engine_strict`. `None` runs the detection here.
    pub early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    /// Per-install packument cache shared with the lockfile-verifier
    /// constructed in [`Install::run`](crate::Install::run). The
    /// resolver writes to it during `pick_package`; the verifier reads
    /// from it to skip duplicate fetches when both touch the same
    /// `(registry, name)`.
    pub meta_cache: Arc<InMemoryPackageMetaCache>,
    /// Resolved [`pnpm_config::Config::node_linker`]. Selects the
    /// materialization shape after the virtual store is populated:
    /// under [`NodeLinker::Hoisted`] the freshly-built lockfile is
    /// routed through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// symlink layout.
    pub node_linker: NodeLinker,
    /// CLI-merged `supportedArchitectures` (`pnpm-workspace.yaml` +
    /// `--cpu`/`--os`/`--libc`). Threaded into the hoisted-linker
    /// walker so its installability filter honors user-supplied
    /// accept lists. `None` when no architectures are configured.
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// When `true`, resolve the graph and write `pnpm-lock.yaml`, then
    /// return — skipping the tarball prefetch, virtual-store
    /// materialization, symlinks, hoisting, and bin linking. The store
    /// stays untouched (no tarball is fetched) — a dry-run resolve pass.
    /// See [`crate::Install::lockfile_only`].
    pub lockfile_only: bool,
    /// `config.skip_runtimes || --no-runtime`; see
    /// [`crate::add_direct_runtime_skips`].
    pub skip_runtimes: bool,
    /// `--dry-run`: build the would-be lockfile but do not write it to
    /// disk. Implies [`Self::lockfile_only`] (nothing is materialized);
    /// the caller diffs the returned [`InstallWithFreshLockfileResult::wanted_lockfile`]
    /// against the existing one and reports the changes.
    pub dry_run: bool,
    /// Whether this invocation can safely read an interactive approval from
    /// stdin. Computed once by the outer install runner from CI and terminal
    /// state, with an explicit override available to deterministic tests.
    pub can_prompt: bool,
    /// Whether resolution-policy bypasses picked during this resolve may be
    /// persisted to `pnpm-workspace.yaml` (today: loose-mode
    /// `minimumReleaseAge` picks appended to `minimumReleaseAgeExclude`).
    /// `true` for the user-facing resolving commands (`install`, `add`,
    /// `update` with `--save`, `dedupe`); `false` for embedder-driven
    /// installs and commands that must not touch the workspace manifest.
    pub persist_policy_excludes: bool,
    /// A full workspace install versus a partial one (`pacquet add` and the
    /// package installs built on it — `dlx`, global add, the engine install).
    /// See [`crate::ProjectMutation::is_full_install`]. Gates the `--no-optional`
    /// exclusion: only a full install's `dependency_groups` carries that
    /// intent, so a partial run must not drop transitive optionals.
    pub is_full_install: bool,
    /// Which lockfile pins to withhold from the preferred-versions seed
    /// so the affected names re-resolve to the highest version
    /// satisfying their manifest range. Drives `pacquet update`'s
    /// compatible bump; see [`UpdateSeedPolicy`].
    pub update_seed_policy: UpdateSeedPolicy,
    /// Preferences layered onto the seed, by package name. `add` / `update`
    /// put a version named on the command line here so the re-resolve lands
    /// on it instead of on the highest one its range allows.
    pub preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    /// Per-invocation `Authorization`-header override; `None` uses
    /// `config.auth_headers`. See [`crate::Install::auth_override`].
    pub auth_override: Option<Arc<AuthHeaders>>,
    /// Sink notified for each resolved tarball package as the tree walk
    /// yields it. `None` for every local install; the pnpr server sets
    /// one. See [`crate::Install::resolution_observer`].
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    /// Out-channel for the resolve's per-importer peer-dependency
    /// issues. See [`crate::Install::peer_issues_sink`].
    pub peer_issues_sink: Option<crate::PeerIssuesSink>,
    /// Out-slot for the dep paths of packages requiring a build. See
    /// [`crate::Install::deps_requiring_build_sink`].
    pub deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    /// In-process `readPackage`/`afterAllResolved` hooks supplied by an
    /// embedder instead of a `.pnpmfile.cjs` on disk. `Some` replaces the
    /// disk lookup entirely; `None` (every CLI install) falls back to
    /// [`load_pnpmfile`][pnpm_hooks::finder::load_pnpmfile]. See [`crate::Install::pnpmfile_hook_override`].
    pub pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub deploy_manifest_hook: bool,
    pub real_importer_ids: Option<&'a std::collections::HashSet<String>>,
    pub selected_importer_ids: Option<&'a std::collections::HashSet<String>>,
    /// What the previous install materialized
    /// (`<virtual_store_dir>/lock.yaml`). Drives the pre-link
    /// [`crate::PruneStaleModules`] reconciliation and the hoisted
    /// linker's previous-graph orphan diff. `None` on a first install.
    pub current_lockfile: Option<&'a Lockfile>,
    /// `hoistedDependencies` recorded by the previous install's
    /// `.modules.yaml`, for [`crate::PruneStaleModules`]'s orphan
    /// hoist-link cleanup. `None` on a first install or when the file
    /// couldn't be fully parsed.
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub prune_orphans: bool,
    /// pnpm's `saveLockfile`: whether the freshly built lockfile may be
    /// written to `<lockfile_dir>/pnpm-lock.yaml`. `false` leaves that
    /// file untouched — the resolved graph is still returned and still
    /// drives `<virtual_store_dir>/lock.yaml`. See
    /// [`crate::Install::run_legacy_deploy`].
    pub save_lockfile: bool,
    /// The declared ranges `pacquet update` asks this run to move onto the
    /// versions it resolves, and the sink it reports them back through.
    /// `None` for every other install.
    pub manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    /// Resolution policies used to validate a filtered repair after the
    /// sanitized merge view has been spliced into the freshly resolved graph.
    pub resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    /// The pre-resolve verification of the existing lockfile, running in
    /// the background while this install resolves and materializes. The
    /// verdict is awaited before bin linking, dependency builds, and the
    /// lockfile save. See [`crate::LockfileVerificationGate`].
    pub lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
}

/// Which lockfile-pinned `(name, version)` pairs to *withhold* from the
/// preferred-versions tie-break seed [`InstallWithFreshLockfile`] builds
/// via `get_preferred_versions_from_lockfile_and_manifests`.
///
/// A name whose pin is withheld no longer carries its previously-locked
/// version at the existing-version weight, so the resolver falls back to
/// picking the highest version satisfying the manifest range — the
/// compatible re-resolution `pacquet update` performs. This is the
/// `update: 'compatible'` resolver mode, which ignores the lockfile
/// version for the dependency being updated.
///
/// `KeepAll` is the install/add default (every pin seeds the table, so
/// unrelated entries keep their resolutions on a rewrite).
///
/// Every withholding variant carries the update's `--depth` ceiling,
/// which bounds how deep the re-resolution reaches: a node past it keeps
/// its locked resolution even when its name is a target. See
/// [`UpdateDepth`].
#[derive(Debug, Default, Clone)]
pub enum UpdateSeedPolicy {
    /// Seed every lockfile pin. `pacquet install` / `pacquet add`.
    #[default]
    KeepAll,
    /// Seed every lockfile pin but re-resolve every dependency edge.
    /// `pacquet dedupe` uses this to preserve valid pins while rebuilding
    /// the graph around the fewest compatible versions.
    KeepAllResolveAll,
    /// Preserve locked versions while regenerating all derived lockfile data.
    FixLockfile,
    /// Re-resolve every registry edge at its locked version using fresh
    /// metadata. `pacquet update --patches` uses this to pick the registry's
    /// current revision without allowing semver movement.
    RefreshRevisions,
    /// Withhold every lockfile pin. `pacquet update` with no package
    /// selectors — the whole graph re-resolves to highest-in-range.
    DropAll {
        max_depth: UpdateDepth,
    },
    /// Withhold only the update targets' pins. `pacquet update <pattern>`
    /// — a matched name re-resolves while everything else keeps its pin,
    /// and a selector that pinned an exact version narrows the target to
    /// that version line. Keyed by package name (scope included); see
    /// [`UpdateTargets`].
    DropOnly {
        targets: UpdateTargets,
        max_depth: UpdateDepth,
    },
    ByImporter {
        policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
        max_depth: UpdateDepth,
    },
}

/// Record `version` as the preferred one for `name`, outranking the pin the
/// lockfile seeds.
///
/// A version named on the command line has to reach the lockfile even when
/// the specifier written to the manifest doesn't carry it — a `catalog:`
/// entry keeps the version in the catalog, so without this the entry's
/// recorded resolution is reused and the request is dropped silently.
pub(crate) fn prefer_requested_version(
    preferred: &mut pnpm_resolving_resolver_base::PreferredVersions,
    name: &str,
    version: &str,
) {
    use pnpm_resolving_resolver_base::{
        EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
        VersionSelectorWithWeight,
    };

    if node_semver::Version::parse(version).is_err() {
        return;
    }
    preferred.entry(name.to_string()).or_default().insert(
        version.to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT + 1,
        }),
    );
}

impl UpdateSeedPolicy {
    /// Withhold every pin at every depth — the re-resolve `pacquet
    /// dedupe` / `pacquet import` and the napi install perform, none of
    /// which expose a `--depth`.
    #[must_use]
    pub fn drop_all() -> Self {
        UpdateSeedPolicy::DropAll { max_depth: UpdateDepth::UNLIMITED }
    }

    fn max_depth(&self) -> UpdateDepth {
        match self {
            UpdateSeedPolicy::KeepAll
            | UpdateSeedPolicy::KeepAllResolveAll
            | UpdateSeedPolicy::FixLockfile
            | UpdateSeedPolicy::RefreshRevisions => UpdateDepth::UNLIMITED,
            UpdateSeedPolicy::DropAll { max_depth }
            | UpdateSeedPolicy::DropOnly { max_depth, .. }
            | UpdateSeedPolicy::ByImporter { max_depth, .. } => *max_depth,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ImporterUpdateSeedPolicy {
    DropAll,
    DropOnly(UpdateTargets),
}

fn update_reuse_scopes(
    policy: &UpdateSeedPolicy,
) -> (
    pnpm_resolving_deps_resolver::UpdateReuseScope,
    BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
) {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    match policy {
        UpdateSeedPolicy::KeepAll => (UpdateReuseScope::All, BTreeMap::new()),
        UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::FixLockfile
        | UpdateSeedPolicy::RefreshRevisions => (UpdateReuseScope::None, BTreeMap::new()),
        UpdateSeedPolicy::DropAll { .. } => (UpdateReuseScope::None, BTreeMap::new()),
        UpdateSeedPolicy::DropOnly { targets, .. } => {
            (UpdateReuseScope::Except(targets.clone()), BTreeMap::new())
        }
        UpdateSeedPolicy::ByImporter { policies, .. } => (
            UpdateReuseScope::All,
            policies
                .iter()
                .map(|(importer_id, policy)| {
                    let scope = match policy {
                        ImporterUpdateSeedPolicy::DropAll => UpdateReuseScope::None,
                        ImporterUpdateSeedPolicy::DropOnly(targets) => {
                            UpdateReuseScope::Except(targets.clone())
                        }
                    };
                    (importer_id.clone(), scope)
                })
                .collect(),
        ),
    }
}

fn full_resolution_required<'a>(
    has_reusable_seed: bool,
    importer_ids: impl IntoIterator<Item = &'a str>,
    default_scope: &pnpm_resolving_deps_resolver::UpdateReuseScope,
    scopes_by_importer: &BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
) -> bool {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    !has_reusable_seed
        || importer_ids.into_iter().all(|importer_id| {
            let scope = if matches!(default_scope, UpdateReuseScope::None) {
                default_scope
            } else {
                scopes_by_importer.get(importer_id).unwrap_or(default_scope)
            };
            matches!(scope, UpdateReuseScope::None)
        })
}

/// Error type of [`InstallWithFreshLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithFreshLockfileError {
    /// A path named by the `pnpmfile` setting is not on disk. pnpm reports the
    /// same code and message from `requireHooks`.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_NOT_FOUND))]
    MissingPnpmfile(#[error(not(source))] pnpm_hooks::finder::MissingPnpmfileError),
    /// The concurrent pre-resolve verification of the existing lockfile
    /// rejected it. The orchestrator maps this back to
    /// `InstallError::LockfileVerification` so the failure keeps the
    /// shape of the eager gates.
    #[diagnostic(transparent)]
    LockfileVerification(#[error(source)] pnpm_lockfile_verification::VerifyError),

    #[diagnostic(transparent)]
    InstallPackageFromRegistry(#[error(source)] InstallPackageFromRegistryError),

    #[diagnostic(transparent)]
    CreateVirtualStore(#[error(source)] CreateVirtualStoreError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    /// Surfaces a failure while removing stale direct-dep or hoist
    /// links during the pre-link reconciliation pass.
    #[diagnostic(transparent)]
    PruneStaleModules(#[error(source)] crate::PruneDirectDepsError),

    #[diagnostic(transparent)]
    LinkPhase(#[error(source)] pnpm_deps_restorer::linking::LinkPhaseError),

    /// Surfaces a failure to cross-link a Bit root component's injected
    /// members into one another's virtual-store slot. Only reachable
    /// when an importer manifest declares
    /// `installConfig.hoistingLimits: "workspaces"`.
    #[diagnostic(transparent)]
    LinkRootComponentMembers(#[error(source)] LinkRootComponentMembersError),

    /// Surfaces failures from [`crate::lockfile_to_hoisted_dep_graph`]
    /// when a fresh install runs under `nodeLinker: hoisted`. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::HoistedDepGraph`.
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] crate::HoistedDepGraphError),

    /// Surfaces failures from [`crate::link_hoisted_modules()`] while
    /// materializing the on-disk hoisted tree on the fresh path. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::LinkHoistedModules`.
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] crate::LinkHoistedModulesError),

    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),

    #[display("failed to write PnP loader: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PNP_FILE))]
    WritePnpFile(#[error(source)] crate::WritePnpFileError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    /// Surfaces a failure to create one of the hoist symlinks
    /// (`<private_hoisted_modules_dir>/<alias>` or
    /// `<public_hoisted_modules_dir>/<alias>`). EEXIST is
    /// already swallowed by the hoist helper, so this only fires
    /// on real I/O failures.
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] crate::SymlinkPackageError),

    /// Surfaces a failure to link bins of privately-hoisted aliases
    /// into the virtual-store-local `<vs>/node_modules/.bin`.
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] LinkBinsError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),

    /// The resolver chain failed for at least one dependency. The
    /// diagnostic is forwarded transparently so a canonical inner code
    /// (e.g. a traversal name's `ERR_PNPM_INVALID_DEPENDENCY_NAME`)
    /// reaches the CLI unchanged. The `Display` still interpolates the
    /// inner error so consumers that stringify the top-level error
    /// (e.g. pnpr's `resolve.rs`, which forwards `err.to_string()` over
    /// the wire) keep the detail.
    #[display("Failed to resolve dependency tree: {_0}")]
    #[diagnostic(transparent)]
    ResolveDependencyTree(#[error(source)] ResolveDependencyTreeError),

    /// Surfaces a failure to read the manifest of a workspace-root
    /// `link:` / `file:` dependency, whose version stands in for the peer
    /// it may satisfy under `resolvePeersFromWorkspaceRoot`.
    #[display("Failed to read the manifest of a workspace root dependency: {_0}")]
    #[diagnostic(transparent)]
    RootDepManifest(#[error(source)] pnpm_package_manifest::PackageManifestError),

    #[display("Failed to build lockfile from resolved dependency graph: {_0}")]
    #[diagnostic(code(pnpm_package_manager::dependencies_graph_to_lockfile))]
    DependenciesGraphToLockfile(#[error(source)] Box<DependenciesGraphToLockfileError>),

    /// `minimumReleaseAgeExclude` patterns rejected at compile time.
    /// Surfaced as `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE`.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    /// `trustPolicyExclude` patterns rejected at compile time.
    /// Surfaced as `ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE`.
    #[display("Invalid value in trustPolicyExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE))]
    TrustPolicyExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    /// `allowBuilds` patterns in `pnpm-workspace.yaml` couldn't be
    /// parsed. Same `VersionPolicyError` shape the frozen-lockfile
    /// path surfaces — see `InstallFrozenLockfileError::VersionPolicy`.
    #[diagnostic(transparent)]
    AllowBuildsPolicy(#[error(source)] VersionPolicyError),

    /// Surfaces any failure from the shared lifecycle-script build
    /// phase — `patchedDependencies` resolution, the `BuildModules`
    /// run, or the post-build top-level bin link. Shared with the
    /// frozen-lockfile path via `run_build_phase`.
    #[diagnostic(transparent)]
    BuildPhase(#[error(source)] crate::install_frozen_lockfile::BuildPhaseError),

    #[diagnostic(transparent)]
    MinimumReleaseAge(#[error(source)] crate::minimum_release_age::MinimumReleaseAgeError),

    /// Surfaces any failure from the fresh-lockfile installability
    /// pass before virtual-store materialization starts.
    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<pnpm_package_is_installable::InstallabilityError>),

    #[diagnostic(transparent)]
    MergeFilteredWantedLockfile(#[error(source)] crate::MergeFilteredWantedLockfileError),

    /// Failed to resolve and hash `patchedDependencies` against the
    /// workspace directory.
    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] pnpm_patching::ResolvePatchedDependenciesError),

    /// Failed to read or hash a patch file when computing the
    /// lockfile's top-level `patchedDependencies` block.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pnpm_patching::CalcPatchHashError),

    /// One or more configured patches were never applied because no
    /// package matched their key. Surfaced as `ERR_PNPM_UNUSED_PATCH`
    /// unless `allowUnusedPatches` is `true`.
    #[diagnostic(transparent)]
    UnusedPatch(#[error(source)] pnpm_patching::UnusedPatchError),

    /// A user-defined `namedRegistries` entry mapped an alias to a
    /// non-http(s) URL. Surfaced at resolver construction so the
    /// install fails fast with a specific error code instead of a
    /// downstream 404. Surfaced as
    /// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(#[error(source)] MergeNamedRegistriesError),

    /// A `packageExtensions` selector's `@<range>` half failed to
    /// parse as a `node-semver` range. A malformed range is rejected
    /// at install start, not at the first per-manifest match, so the
    /// user sees the bad selector before any tarballs are fetched.
    #[diagnostic(transparent)]
    InvalidPackageExtensionSelector(
        #[error(source)] crate::package_extender::InvalidPackageExtensionSelector,
    ),

    /// A value in `pnpm.overrides` couldn't be parsed before the
    /// fresh resolver's read-package hook was built.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pnpm_config_parse_overrides::ParseOverridesError),

    /// The first writer of a shared `(name, version)` slot dropped its
    /// completion signal without sending `true`. In practice this only
    /// fires when the first writer's task panicked / was cancelled
    /// mid-import; a second visitor that was waiting on the slot can't
    /// safely create its per-parent symlink (the virtual-store target
    /// directory may not exist), so the install fails closed.
    #[display(
        "First writer for virtual-store slot {virtual_store_name} dropped before signalling completion"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_FIRST_WRITER_ABORTED))]
    FirstWriterAborted {
        #[error(not(source))]
        virtual_store_name: String,
    },

    /// Persisting the freshly-resolved `pnpm-lock.yaml` failed. Surfaced
    /// rather than swallowed because a missing wanted lockfile would
    /// force the next install to re-resolve every dep and would break
    /// the `pnpm install --frozen-lockfile` headless path.
    #[diagnostic(transparent)]
    SaveWantedLockfile(#[error(source)] SaveLockfileError),

    /// The `afterAllResolved` pnpmfile hook threw or otherwise failed.
    /// A throwing `afterAllResolved` aborts the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    AfterAllResolvedHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The freshly-built lockfile could not be serialized to JSON to pass to
    /// the `afterAllResolved` pnpmfile hook.
    #[display("Failed to serialize lockfile for the afterAllResolved hook: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_AFTER_ALL_RESOLVED_SERIALIZE))]
    AfterAllResolvedSerialize(#[error(source)] serde_json::Error),

    /// The pnpmfile's `getCustomResolvers` hook threw while loading custom
    /// resolvers. A throwing custom-resolver hook aborts the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The pnpmfile threw while loading its custom `fetchers` export.
    /// Same fatality rule as [`Self::CustomResolverHook`] and the
    /// frozen-lockfile path's custom-fetcher load.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomFetcherHook(#[error(not(source))] pnpm_hooks::HookError),

    /// A custom resolver's `shouldRefreshResolution` hook threw while
    /// checking whether to force re-resolution. A throwing hook aborts
    /// the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverForceResolve(#[error(not(source))] pnpm_hooks::HookError),
}

impl From<crate::install_frozen_lockfile::HoistedLinkerError> for InstallWithFreshLockfileError {
    fn from(error: crate::install_frozen_lockfile::HoistedLinkerError) -> Self {
        use crate::install_frozen_lockfile::HoistedLinkerError;
        match error {
            HoistedLinkerError::HoistedDepGraph(error) => {
                InstallWithFreshLockfileError::HoistedDepGraph(error)
            }
            HoistedLinkerError::LinkHoistedModules(error) => {
                InstallWithFreshLockfileError::LinkHoistedModules(error)
            }
            HoistedLinkerError::SymlinkDirectDependencies(error) => {
                InstallWithFreshLockfileError::SymlinkDirectDependencies(error)
            }
            HoistedLinkerError::WritePackageMap(error) => {
                InstallWithFreshLockfileError::WritePackageMap(error)
            }
        }
    }
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
/// The install's borrowed inputs, as one `Copy` value the phases read.
#[derive(Clone, Copy)]
struct FreshInputs<'a> {
    http_client: &'a ThrottledClient,
    config: &'static Config,
    dependency_groups: &'a [DependencyGroup],
    logged_methods: &'a AtomicU8,
    requester: &'a str,
    lockfile_dir: &'a Path,
    update_checksums: bool,
    wanted_lockfile: Option<&'a Lockfile>,
    merge_wanted_lockfile: Option<&'a Lockfile>,
    node_linker: NodeLinker,
    supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    lockfile_only: bool,
    skip_runtimes: bool,
    dry_run: bool,
    can_prompt: bool,
    persist_policy_excludes: bool,
    is_full_install: bool,
    deploy_manifest_hook: bool,
    real_importer_ids: Option<&'a std::collections::HashSet<String>>,
    selected_importer_ids: Option<&'a std::collections::HashSet<String>>,
    current_lockfile: Option<&'a Lockfile>,
    prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    prune_orphans: bool,
    save_lockfile: bool,
    manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
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
struct OwnedInputs {
    update_seed_policy: UpdateSeedPolicy,
    tarball_mem_cache: Arc<MemCache>,
    http_client_arc: Arc<ThrottledClient>,
    lockfile_specifier_manifests: Option<BTreeMap<String, PackageManifest>>,
    catalogs: Catalogs,
    workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    wanted_lockfile_shared: Option<Arc<Lockfile>>,
    node_version: Option<String>,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    meta_cache: Arc<InMemoryPackageMetaCache>,
    preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    auth_override: Option<Arc<AuthHeaders>>,
    resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    peer_issues_sink: Option<crate::PeerIssuesSink>,
    deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
}

impl<'a> InstallWithFreshLockfile<'a> {
    /// Separate what the phases borrow from what one of them consumes.
    fn split(self) -> (FreshInputs<'a>, OwnedInputs, ManifestSlots<'a>) {
        (
            FreshInputs {
                http_client: self.http_client,
                config: self.config,
                dependency_groups: self.dependency_groups,
                logged_methods: self.logged_methods,
                requester: self.requester,
                lockfile_dir: self.lockfile_dir,
                update_checksums: self.update_checksums,
                wanted_lockfile: self.wanted_lockfile,
                merge_wanted_lockfile: self.merge_wanted_lockfile,
                node_linker: self.node_linker,
                supported_architectures: self.supported_architectures,
                lockfile_only: self.lockfile_only,
                skip_runtimes: self.skip_runtimes,
                dry_run: self.dry_run,
                can_prompt: self.can_prompt,
                persist_policy_excludes: self.persist_policy_excludes,
                is_full_install: self.is_full_install,
                deploy_manifest_hook: self.deploy_manifest_hook,
                real_importer_ids: self.real_importer_ids,
                selected_importer_ids: self.selected_importer_ids,
                current_lockfile: self.current_lockfile,
                prior_hoisted_dependencies: self.prior_hoisted_dependencies,
                prune_orphans: self.prune_orphans,
                save_lockfile: self.save_lockfile,
                manifest_spec_bumps: self.manifest_spec_bumps,
                resolution_verifiers: self.resolution_verifiers,
            },
            OwnedInputs {
                update_seed_policy: self.update_seed_policy,
                tarball_mem_cache: self.tarball_mem_cache,
                http_client_arc: self.http_client_arc,
                lockfile_specifier_manifests: self.lockfile_specifier_manifests,
                catalogs: self.catalogs,
                workspace_packages: self.workspace_packages,
                wanted_lockfile_shared: self.wanted_lockfile_shared,
                node_version: self.node_version,
                early_host_detection: self.early_host_detection,
                meta_cache: self.meta_cache,
                preferred_versions_override: self.preferred_versions_override,
                auth_override: self.auth_override,
                resolution_observer: self.resolution_observer,
                peer_issues_sink: self.peer_issues_sink,
                deps_requiring_build_sink: self.deps_requiring_build_sink,
                pnpmfile_hook_override: self.pnpmfile_hook_override,
                lockfile_verification_gate: self.lockfile_verification_gate,
            },
            ManifestSlots::declared(self.importer_manifests),
        )
    }
}

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
        let (install, mut owned, mut manifests) = self.split();
        let mut setup = set_up_resolvers::<Reporter>(install, &mut owned).await?;
        let mut resolved =
            resolve_graph::<Reporter>(install, &mut owned, &mut setup, &mut manifests).await?;
        if resolved.full_resolution {
            warn_stale_convergence_overrides_if_any::<Reporter>(
                &*setup.chain.npm_resolver,
                resolved.parsed_overrides.as_deref(),
                resolved.versions_overrider.as_deref(),
                install.lockfile_dir,
                (setup.policy.published_by, setup.policy.published_by_exclude.as_ref()),
            )
            .await;
        }
        // Resolution is over: release the resolvers and the metadata
        // cache before materializing, so their memory does not outlive
        // its use. The custom fetchers stay, the cold batch consults them.
        drop(setup.chain.resolver);
        drop(setup.chain.npm_resolver);
        drop(owned.meta_cache);
        drop(setup.chain.fetch_locker);
        drop(setup.chain.picked_manifest_cache);
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: install.lockfile_dir.display().to_string(),
            stage: Stage::ResolutionDone,
        }));
        let allow_build_policy = (!install.lockfile_only)
            .then(|| AllowBuildPolicy::from_config(install.config))
            .transpose()
            .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)?;
        let built_lockfile = build_lockfile_phase::<Reporter>(
            install,
            &mut owned.lockfile_verification_gate,
            std::mem::take(&mut resolved.time),
            &resolved,
            LockfileViews {
                importer_manifests: &resolved.importer_manifests,
                wanted_lockfile: resolved
                    .fixed_wanted_lockfile
                    .as_ref()
                    .or(install.wanted_lockfile),
                catalogs: &owned.catalogs,
                lockfile_specifier_manifests: owned.lockfile_specifier_manifests.as_ref(),
            },
            setup.shape.verify_filtered_repair,
        )
        .await?;
        let Some(allow_build_policy) = allow_build_policy else {
            return finish_lockfile_only::<Reporter>(LockfileOnlyOptions {
                built_lockfile,
                peer_issue_importer_ids: resolved.peer_issue_importer_ids,
                config: install.config,
                lockfile_dir: install.lockfile_dir,
                requester: install.requester,
                dry_run: install.dry_run,
                save_lockfile: install.save_lockfile,
                after_all_resolved_hook: resolved.after_all_resolved_hook.as_ref(),
                after_all_resolved_log: resolved.after_all_resolved_log,
                store_index_writer: setup.stores.writer,
                writer_task: setup.stores.writer_task,
            })
            .await;
        };
        let initial =
            MaterializationScope::initial(install, setup.shape.is_hoisted, &built_lockfile);
        let mut plan = plan_fresh_materialization::<Reporter>(
            install,
            HostProbeInputs {
                early_host_detection: owned.early_host_detection.take(),
                node_version: owned.node_version.take(),
            },
            PlanLockfiles { initial: initial.lockfile(&built_lockfile), built: &built_lockfile },
            &allow_build_policy,
            PlanScope {
                included: install.included(),
                include_transitive_optional_dependencies: setup
                    .shape
                    .include_transitive_optional_dependencies,
            },
        )
        .await?;
        let scope =
            initial.finalize(install, setup.shape.is_hoisted, &built_lockfile, &plan.skipped);
        finish_early_materialization(
            resolved.early_materializer.as_deref(),
            initial.lockfile(&built_lockfile).snapshots.as_ref(),
            &plan.skipped,
            install.logged_methods,
        )
        .await;
        let on_disk = run_on_disk_phases::<Reporter>(
            OnDiskInputs {
                ctx: &pnpm_deps_restorer::InstallContext {
                    config: install.config,
                    workspace_root: install.lockfile_dir,
                    requester: install.requester,
                    layout: &plan.layout,
                    node_linker: install.node_linker,
                    allow_build_policy: &allow_build_policy,
                    link_options: &setup.shape.link_options,
                    logged_methods: install.logged_methods,
                },
                http_client: install.http_client,
                prune_orphans: install.prune_orphans,
                include_transitive_optional_dependencies: setup
                    .shape
                    .include_transitive_optional_dependencies,
                supported_architectures: install.supported_architectures,
                current_lockfile: install.current_lockfile,
                prior_hoisted_dependencies: install.prior_hoisted_dependencies,
                deps_requiring_build_sink: owned.deps_requiring_build_sink,
                tarball_mem_cache: &owned.tarball_mem_cache,
                materialization_lockfile: scope.lockfile(&built_lockfile),
                importer_manifests: &resolved.importer_manifests,
                dependency_groups: install.dependency_groups,
                project_anchor_importer_ids: &scope.project_anchor_importer_ids,
                dir_clone_cache: plan.dir_clone_cache.as_ref(),
                host_node: plan.host_node.as_ref(),
                engine_name: plan.engine_name,
                deferred_engine_name: plan.deferred_engine_name,
                patched_dependencies: resolved.patched_dependencies.as_deref(),
                custom_fetcher_session: setup.chain.custom_fetcher_session.as_ref(),
                store_index_ref: setup.stores.index.as_ref(),
                store_index_writer: setup.stores.writer,
                caches: &setup.stores.caches,
            },
            &mut plan.skipped,
            &mut owned.lockfile_verification_gate,
        )
        .await?;
        let persisted = persist_fresh_lockfile(
            built_lockfile,
            install.config,
            install.lockfile_dir,
            install.save_lockfile,
            (resolved.after_all_resolved_hook.as_ref(), resolved.after_all_resolved_log.clone()),
        )
        .await?;
        Ok(InstallWithFreshLockfileResult {
            hoisted_dependencies: on_disk.hoisted_dependencies,
            hoisted_locations: on_disk.hoisted_locations,
            injected_deps: on_disk.injected_deps,
            peer_issue_importer_ids: resolved.peer_issue_importer_ids,
            wanted_lockfile: persisted.lockfile,
            can_record_lockfile_verification: persisted.can_record_lockfile_verification,
            ignored_builds: on_disk.ignored_builds,
            deferred_builds: on_disk.deferred_builds,
            skipped: on_disk.skipped,
            store_index_teardown: setup.stores.writer_task,
        })
    }
}

/// What the setup phase hands the resolve phase: the registries, the
/// pick policy, the store handles and the resolver chain, plus what the
/// inputs and the resolution observer fix for the whole install.
struct ResolverSetup {
    workspace_packages: Option<Arc<pnpm_resolving_resolver_base::WorkspacePackages>>,
    observer: ObserverSettings,
    shape: InstallShape,
    policy: crate::resolution_policy::PickPolicy,
    registries: resolver_setup::Registries,
    stores: resolver_setup::StoreIndexHandles,
    chain: resolver_setup::ResolverChain,
}

/// What the inputs fix about the install before anything resolves.
struct InstallShape {
    is_hoisted: bool,
    link_options: pnpm_cmd_shim::LinkBinsOptions,
    filtered_isolated: bool,
    verify_filtered_repair: bool,
    include_transitive_optional_dependencies: bool,
}

impl InstallShape {
    fn derive(install: FreshInputs<'_>, update_seed_policy: &UpdateSeedPolicy) -> Self {
        let is_hoisted = matches!(install.node_linker, NodeLinker::Hoisted);
        let partial_selection = is_partial_workspace_selection(
            install.real_importer_ids,
            install.selected_importer_ids,
        );
        Self {
            is_hoisted,
            link_options: crate::shim_link_options(install.config, install.node_linker),
            filtered_isolated: partial_selection && !is_hoisted,
            verify_filtered_repair: matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile)
                && partial_selection,
            include_transitive_optional_dependencies: include_transitive_optional_dependencies(
                install.is_full_install,
                install.dependency_groups,
            ),
        }
    }
}

/// What the resolution observer fixes for the resolvers when one drives
/// the install. Overrides cannot take the fast update while an observer
/// must see every resolution.
struct ObserverSettings {
    package_version_guard: Option<Arc<dyn pnpm_resolving_resolver_base::PackageVersionGuard>>,
    minimum_release_age_exclude_override: Option<Vec<String>>,
    can_fast_update_overrides: bool,
}

impl ObserverSettings {
    fn of(observer: Option<&Arc<dyn crate::ResolutionObserver>>) -> Self {
        Self {
            package_version_guard: observer.and_then(|observer| observer.package_version_guard()),
            minimum_release_age_exclude_override: observer
                .and_then(|observer| observer.minimum_release_age_exclude_override()),
            can_fast_update_overrides: observer.is_none(),
        }
    }
}

/// Open the store, resolve the registries and build the resolver chain.
/// Consumes the auth override, the resolution observer, the workspace
/// packages and the pnpmfile override off `owned`.
async fn set_up_resolvers<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    owned: &mut OwnedInputs,
) -> Result<ResolverSetup, InstallWithFreshLockfileError> {
    let shape = InstallShape::derive(install, &owned.update_seed_policy);
    // The pnpr override when supplied, else the config's npmrc headers;
    // shared by every registry-touching resolver below.
    let auth_headers =
        owned.auth_override.take().unwrap_or_else(|| Arc::clone(&install.config.auth_headers));
    let resolution_observer = owned.resolution_observer.take();
    let observer = ObserverSettings::of(resolution_observer.as_ref());

    let store_dir: &'static _ = &install.config.store_dir;
    // Eagerly create `files/00..ff` under the v11 store root so per-
    // tarball CAFS writes never pay a `create_dir_all` syscall on the
    // hot path.
    // See [`init_store_dir_best_effort`] for the error-degradation
    // policy shared with `create_virtual_store.rs`. Skipped under
    // `frozenStore`: the store is read-only and complete, so no
    // directory creation is attempted under its root.
    if !install.config.frozen_store {
        init_store_dir_best_effort(store_dir).await;
    }

    let registries = resolver_setup::resolve_registries(install.config)?;

    // `resolutionMode` / `minimumReleaseAge` derivations. `time_based`
    // and `pick_lowest_direct` steer the deps-resolver's per-depth
    // version pick; `full_metadata` forces the npm resolver to fetch
    // per-version `time` fields so the time-based cutoff and the
    // no-downgrade trust check have publication dates; `published_by`
    // (+exclude) is the maturity cutoff. Shared with `pacquet add`'s
    // explicit-spec pre-resolution via [`PickPolicy`] so both pick the
    // same version.
    let policy = crate::resolution_policy::PickPolicy::from_config_with_extra_excludes(
        install.config,
        observer.minimum_release_age_exclude_override.as_deref(),
    )
    .map_err(InstallWithFreshLockfileError::MinimumReleaseAgeExclude)?;

    // `caches` records, among other things, the package-status
    // progress emitted by resolve-time prefetches: `CreateVirtualStore`
    // still emits `resolved` later, but skips duplicate `fetched` /
    // `found_in_store` statuses for keys already reported here.
    let stores = resolver_setup::open_store_index_handles(install.config, store_dir).await;

    let chain =
        resolver_setup::build_resolver_chain::<Reporter>(resolver_setup::ResolverChainInputs {
            config: install.config,
            store_dir,
            http_client_arc: &owned.http_client_arc,
            tarball_mem_cache: &owned.tarball_mem_cache,
            auth_headers: &auth_headers,
            meta_cache: &owned.meta_cache,
            lockfile_dir: install.lockfile_dir,
            requester: install.requester,
            supported_architectures: install.supported_architectures,
            registries: &registries.by_scope,
            needs_full_metadata_for: Arc::clone(&policy.needs_full_metadata_for),
            registries_by_prefix: &registries.named,
            full_metadata: policy.full_metadata,
            wanted_lockfile: install.wanted_lockfile,
            store_index: stores.index.as_ref(),
            store_index_writer: &stores.writer,
            verified_files_cache: &stores.caches.verified_files,
            progress_reported: &stores.caches.progress_reported,
            prefetch_downloads: prefetch_downloads(install.lockfile_only, shape.filtered_isolated),
            pnpmfile_hook_override: owned.pnpmfile_hook_override.take(),
            resolution_observer,
        })
        .await?;
    Ok(ResolverSetup {
        workspace_packages: owned.workspace_packages.take().map(Arc::new),
        observer,
        shape,
        policy,
        registries,
        stores,
        chain,
    })
}

/// What the resolve phase leaves for the lockfile and the on-disk phases.
///
/// The importer manifests view borrows the slots `run` owns; the repaired
/// wanted lockfile under `fix-lockfile` is owned here and `run` derives
/// its view after the phase returns.
struct Resolved<'a, Reporter> {
    early_materializer: Option<Arc<crate::early_materializer::EarlyMaterializer<Reporter>>>,
    parsed_overrides: Option<Vec<pnpm_config_parse_overrides::VersionOverride>>,
    overrides: Option<IndexMap<String, String>>,
    versions_overrider: Option<Arc<crate::VersionsOverrider>>,
    importer_manifests: ManifestsView<'a>,
    fixed_wanted_lockfile: Option<Lockfile>,
    patched_dependencies: Option<Arc<pnpm_patching::PatchGroupRecord>>,
    patched_dependency_hashes: Option<BTreeMap<String, String>>,
    after_all_resolved_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    after_all_resolved_log: Option<pnpm_hooks::LogFn>,
    guard_previous_importers: Option<&'a HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    guard_update_reuse_scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    guard_update_reuse_scopes_by_importer:
        BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
    full_resolution: bool,
    peer_issue_importer_ids: HashSet<String>,
    merged_graph: pnpm_resolving_deps_resolver::DependenciesGraph,
    direct_by_importer: BTreeMap<String, BTreeMap<String, pnpm_deps_path::DepPath>>,
    time: BTreeMap<String, String>,
}

/// The importer manifests as declared, and as the transforms rewrote them
/// when any applied. `run` owns them so the resolve phase can hand back
/// one view that every later phase reads.
struct ManifestSlots<'a> {
    declared: BTreeMap<String, &'a PackageManifest>,
    effective: BTreeMap<String, PackageManifest>,
}

/// The importer manifests as the resolver sees them: the transforms'
/// rewrites when they made any, the declared manifests otherwise.
type ManifestsView<'m> = std::borrow::Cow<'m, BTreeMap<String, &'m PackageManifest>>;

impl<'a> ManifestSlots<'a> {
    fn declared(declared: BTreeMap<String, &'a PackageManifest>) -> Self {
        Self { declared, effective: BTreeMap::new() }
    }

    /// Build the read-package hook chain and rewrite every importer's
    /// manifest through it.
    fn transform(
        &mut self,
        config: &Config,
        catalogs: &Catalogs,
        lockfile_dir: &Path,
        deploy_manifest_hook: bool,
    ) -> Result<manifest_transforms::ManifestTransforms, InstallWithFreshLockfileError> {
        let mut transforms = manifest_transforms::build_manifest_transforms(
            config,
            catalogs,
            lockfile_dir,
            &self.declared,
            deploy_manifest_hook,
        )?;
        self.effective = std::mem::take(&mut transforms.effective_importer_manifests);
        Ok(transforms)
    }

    /// Borrows in the common case; allocates only the map of references
    /// when a transform rewrote manifests.
    fn view(&self) -> ManifestsView<'_> {
        if self.effective.is_empty() {
            std::borrow::Cow::Borrowed(&self.declared)
        } else {
            std::borrow::Cow::Owned(
                self.effective.iter().map(|(id, manifest)| (id.clone(), manifest)).collect(),
            )
        }
    }
}

/// Resolve every importer's dependency graph. Consumes the registries,
/// the pnpmfile hook and the shared wanted lockfile.
async fn resolve_graph<'a: 'm, 'm, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    owned: &mut OwnedInputs,
    setup: &mut ResolverSetup,
    manifests: &'m mut ManifestSlots<'a>,
) -> Result<Resolved<'m, Reporter>, InstallWithFreshLockfileError> {
    let registries = std::mem::take(&mut setup.registries);
    let prep = prepare_resolution::<Reporter>(install, owned, setup, manifests).await?;
    let importer_manifests = manifests.view();
    let wanted_lockfile = prep.fixed_wanted_lockfile.as_ref().or(install.wanted_lockfile);
    let (preferred_versions_seed, preferred_versions_seeds_by_importer) =
        resolve::preferred_versions_seeds(
            &owned.update_seed_policy,
            wanted_lockfile,
            &importer_manifests,
            owned.preferred_versions_override.as_ref(),
        );
    let shared_resolve_options = resolve::SharedResolveOptions {
        config: install.config,
        lockfile_dir: install.lockfile_dir,
        published_by: setup.policy.published_by,
        published_by_exclude: setup.policy.published_by_exclude.clone(),
        trust_policy: prep.trust.policy,
        trust_policy_exclude: prep.trust.exclude.clone(),
        package_version_guard: setup.observer.package_version_guard.clone(),
        workspace_packages: setup.workspace_packages.clone(),
        update_checksums: install.update_checksums,
        update_behavior: resolver_update_behavior(&owned.update_seed_policy),
    };
    let lockfile_reuse_seed = resolve::lockfile_reuse_seed(resolve::ReuseSeedInputs {
        config: install.config,
        catalogs: &owned.catalogs,
        wanted_lockfile,
        wanted_lockfile_shared: prep.wanted_lockfile_shared.as_ref(),
        package_extensions_checksum: prep.transforms.package_extensions_checksum.as_deref(),
        parsed_overrides: prep.transforms.parsed_overrides.as_deref(),
        resolved_overrides: prep.transforms.resolved_overrides.as_ref(),
        manifest_hook: prep.transforms.hooks.manifest_hook.clone(),
        overrides_hook: prep.transforms.hooks.overrides_hook.clone(),
        fast_override_eligible: fast_override_eligible(FastOverrideFit {
            has_pnpmfile_hook: prep.hooks.pnpmfile_hook.is_some(),
            has_custom_resolvers: !setup.chain.custom_resolvers.is_empty(),
            has_patches: prep.patches.record.is_some(),
            can_fast_update_overrides: setup.observer.can_fast_update_overrides,
        }),
        npm_resolver: &*setup.chain.npm_resolver,
        resolve_options: &shared_resolve_options
            .build(install.lockfile_dir.to_path_buf(), Arc::clone(&preferred_versions_seed)),
        registries: &registries.by_scope,
    })
    .await;
    let phase_start = std::time::Instant::now();
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: install.lockfile_dir.display().to_string(),
        stage: Stage::ResolutionStarted,
    }));
    let workspace_result = resolve::run_resolve_pass::<Reporter>(resolve::ResolvePassInputs {
        resolver: &*setup.chain.resolver,
        importer_manifests: &importer_manifests,
        dependency_groups: install.dependency_groups,
        walk: resolve::WorkspaceWalk {
            share_workspace_resolutions: setup.chain.custom_resolvers.is_empty(),
            pnpmfile_hook: prep.hooks.pnpmfile_hook.clone(),
            read_package_log: prep.hooks.read_package_log.clone(),
            finalized_package: prep
                .early_materializer
                .as_ref()
                .map(crate::early_materializer::EarlyMaterializer::hook),
            time_based: setup.policy.time_based,
            resolution_lockfile: lockfile_reuse_seed
                .clone()
                .or_else(|| prep.wanted_lockfile_shared.clone())
                .or_else(|| wanted_lockfile.cloned().map(Arc::new)),
            reuse_lockfile_subtrees: lockfile_reuse_seed.is_some(),
            update_reuse_scope: prep.reuse.scope.clone(),
            update_reuse_scopes_by_importer: prep.reuse.by_importer.clone(),
            update_depth: owned.update_seed_policy.max_depth(),
            registries_by_prefix: registries.named.clone(),
            registries: registries.by_scope,
        },
        per_importer: resolve::ImporterInputs {
            config: install.config,
            catalogs: &owned.catalogs,
            lockfile_dir: install.lockfile_dir,
            shared_resolve_options: &shared_resolve_options,
            preferred_versions_seed: &preferred_versions_seed,
            preferred_versions_seeds_by_importer: &preferred_versions_seeds_by_importer,
            override_bare_specifier: prep.transforms.hooks.override_bare_specifier.clone(),
            patched_dependencies: prep.patches.record.clone(),
            manifest_hook: prep.transforms.hooks.manifest_hook.clone(),
            overrides_hook: prep.transforms.hooks.overrides_hook.clone(),
            pick_lowest_direct: setup.policy.pick_lowest_direct,
            published_by: setup.policy.published_by,
        },
    })
    .await?;
    let pass = ResolvePass {
        result: workspace_result,
        full_resolution: full_resolution_required(
            lockfile_reuse_seed.is_some(),
            importer_manifests.keys().map(String::as_str),
            &prep.reuse.scope,
            &prep.reuse.by_importer,
        ),
        started: phase_start,
        linked_peer_importers: importers_consuming_linked_peers(
            &importer_manifests,
            install.lockfile_dir,
        ),
        importer_manifests,
    };
    collect_resolution::<Reporter>(install, owned.peer_issues_sink.as_ref(), prep, pass).await
}

/// A finished resolve pass, with what the phase around it decided.
struct ResolvePass<'m> {
    result: pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    full_resolution: bool,
    started: std::time::Instant,
    /// Importers whose linked packages consume peers.
    linked_peer_importers: HashSet<String>,
    importer_manifests: ManifestsView<'m>,
}

/// Enforce the policies the pass reports against, gather the peer
/// issues, and assemble the phase's output.
async fn collect_resolution<'m, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'m>,
    peer_issues_sink: Option<&crate::PeerIssuesSink>,
    prep: ResolutionPrep<Reporter>,
    pass: ResolvePass<'m>,
) -> Result<Resolved<'m, Reporter>, InstallWithFreshLockfileError> {
    let ResolvePass {
        result: workspace_result,
        full_resolution,
        started,
        linked_peer_importers,
        importer_manifests,
    } = pass;
    let (can_prompt_now, persist_policy_excludes_now) =
        interactive_policy(install.can_prompt, install.persist_policy_excludes, install.dry_run);
    crate::minimum_release_age::handle_minimum_release_age_violations::<Reporter>(
        install.config,
        install.lockfile_dir,
        &workspace_result.merged_tree.policy_violations,
        can_prompt_now,
        persist_policy_excludes_now,
    )
    .await
    .map_err(InstallWithFreshLockfileError::MinimumReleaseAge)?;
    check_patch_usage::<Reporter>(
        install.config,
        prep.patches.record.as_deref(),
        &workspace_result.merged_tree.applied_patches,
        PatchUsageScope {
            real_importer_ids: install.real_importer_ids,
            selected_importer_ids: install.selected_importer_ids,
            merge_wanted_lockfile: install.merge_wanted_lockfile,
        },
    )?;
    let mut peer_issue_importer_ids: HashSet<String> =
        workspace_result.peers.peer_dependency_issues_by_importer.keys().cloned().collect();
    peer_issue_importer_ids.extend(linked_peer_importers);
    report_peer_issues(
        peer_issues_sink,
        &workspace_result.peers.peer_dependency_issues_by_importer,
    );
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "resolve_workspace",
        elapsed_ms = started.elapsed().as_millis() as u64,
        importers = importer_manifests.len(),
        nodes = workspace_result.peers.graph.len(),
        "phase complete",
    );
    Ok(Resolved {
        early_materializer: prep.early_materializer,
        parsed_overrides: prep.transforms.parsed_overrides,
        overrides: prep.transforms.resolved_overrides,
        versions_overrider: prep.transforms.versions_overrider,
        importer_manifests,
        fixed_wanted_lockfile: prep.fixed_wanted_lockfile,
        patched_dependencies: prep.patches.record,
        patched_dependency_hashes: prep.patches.hashes,
        after_all_resolved_hook: prep.hooks.pnpmfile_hook,
        after_all_resolved_log: prep.hooks.after_all_resolved_log,
        guard_previous_importers: install
            .merge_wanted_lockfile
            .filter(|_| install.config.dedupe_injected_deps)
            .map(|lockfile| &lockfile.importers),
        guard_update_reuse_scope: prep.reuse.scope,
        guard_update_reuse_scopes_by_importer: prep.reuse.by_importer,
        full_resolution,
        peer_issue_importer_ids,
        merged_graph: workspace_result.peers.graph,
        direct_by_importer: workspace_result.peers.direct_dependencies_by_importer,
        time: workspace_result.time,
    })
}

/// The pnpmfile's hooks and the loggers that report their use.
struct PnpmfileHooks {
    pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    read_package_log: Option<pnpm_hooks::LogFn>,
    after_all_resolved_log: Option<pnpm_hooks::LogFn>,
}

impl PnpmfileHooks {
    async fn run_pre_resolution<Reporter: self::Reporter>(
        &self,
        config: &Config,
        lockfile_dir: &Path,
        wanted_lockfile: Option<&Lockfile>,
    ) {
        if let Some(hook) = self.pnpmfile_hook.as_ref() {
            resolve::run_pre_resolution_hook::<Reporter>(
                hook,
                config,
                lockfile_dir,
                wanted_lockfile,
            )
            .await;
        }
    }

    fn load<Reporter: self::Reporter>(
        pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        lockfile_dir: &Path,
    ) -> Self {
        let path =
            pnpmfile_hook.as_ref().and_then(|hook| hook.source_path()).map(Path::to_path_buf);
        let log = |name: &'static str| {
            path.as_ref().map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, name))
        };
        PnpmfileHooks {
            read_package_log: log("readPackage"),
            after_all_resolved_log: log("afterAllResolved"),
            pnpmfile_hook,
        }
    }
}

/// What resolution reads that is settled before the resolve pass runs.
///
/// Two views the pass reads borrow values this struct owns: the
/// manifests as the pnpmfile rewrote them, and the wanted lockfile as
/// `fix-lockfile` repaired it.
struct ResolutionPrep<Reporter> {
    early_materializer: Option<Arc<crate::early_materializer::EarlyMaterializer<Reporter>>>,
    trust: TrustGate,
    transforms: manifest_transforms::ManifestTransforms,
    fixed_wanted_lockfile: Option<Lockfile>,
    wanted_lockfile_shared: Option<Arc<Lockfile>>,
    patches: Patches,
    hooks: PnpmfileHooks,
    reuse: UpdateReuseScopes,
}

/// The `trustPolicy='no-downgrade'` gate, threaded into every resolve so
/// the npm resolver re-applies it to freshly picked versions. Full
/// metadata is forced on under the policy, so the picker hands the
/// resolver the per-version `time` and trust evidence the check reads.
struct TrustGate {
    policy: Option<TrustPolicy>,
    exclude: Option<pnpm_config::version_policy::PackageVersionPolicy>,
}

impl TrustGate {
    fn of(config: &Config) -> Result<Self, InstallWithFreshLockfileError> {
        Ok(Self {
            policy: resolver_trust_policy(config.trust_policy),
            exclude: config
                .trust_policy_exclude
                .as_deref()
                .filter(|patterns| !patterns.is_empty())
                .map(pnpm_config::version_policy::create_package_version_policy)
                .transpose()
                .map_err(InstallWithFreshLockfileError::TrustPolicyExclude)?,
        })
    }
}

/// `pnpm-workspace.yaml`'s `patchedDependencies`, resolved once per
/// install: the record grouped by package name that the resolver
/// consults at every per-node lookup to attach `(patch_hash=<hash>)` to
/// the matched package's `pkgIdWithPatchHash`, and the user's verbatim
/// keys mapped to their patch-file hashes for the lockfile's top-level
/// `patchedDependencies` block.
struct Patches {
    record: Option<Arc<pnpm_patching::PatchGroupRecord>>,
    hashes: Option<BTreeMap<String, String>>,
}

impl Patches {
    fn resolve(config: &Config) -> Result<Self, InstallWithFreshLockfileError> {
        Ok(Self {
            record: config
                .resolved_patched_dependencies()
                .map_err(InstallWithFreshLockfileError::ResolvePatchedDependencies)?
                .map(Arc::new),
            hashes: config
                .patched_dependency_hashes()
                .map_err(InstallWithFreshLockfileError::CalcPatchHashes)?,
        })
    }
}

/// Where lockfile reuse is suppressed: `pacquet update` must re-resolve
/// its targets (and their subtrees) to highest-in-range, and a custom
/// resolver may widen the scope to everything via
/// `shouldRefreshResolution`.
struct UpdateReuseScopes {
    scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    by_importer: BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
}

impl UpdateReuseScopes {
    /// A throwing `shouldRefreshResolution` hook propagates and aborts.
    async fn settle(
        update_seed_policy: &UpdateSeedPolicy,
        custom_resolvers: &[Arc<dyn pnpm_hooks::CustomResolver>],
        wanted_lockfile: Option<&Lockfile>,
    ) -> Result<Self, InstallWithFreshLockfileError> {
        let (mut scope, mut by_importer) = update_reuse_scopes(update_seed_policy);
        if custom_resolver_forces_resolve(custom_resolvers, wanted_lockfile).await? {
            scope = pnpm_resolving_deps_resolver::UpdateReuseScope::None;
            by_importer.clear();
        }
        Ok(Self { scope, by_importer })
    }
}

/// Runs between the resolvers being built and the resolve pass, in the
/// order pnpm's install applies these: the pnpmfile's pre-resolution hook
/// fires once the lockfile to resolve against is fixed, and a custom
/// resolver may still widen the reuse scopes after that.
async fn prepare_resolution<'a, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    owned: &mut OwnedInputs,
    setup: &mut ResolverSetup,
    manifests: &mut ManifestSlots<'a>,
) -> Result<ResolutionPrep<Reporter>, InstallWithFreshLockfileError> {
    let early_materializer = early_materialization_eligible(EarlyMaterializationFit {
        config: install.config,
        node_linker: install.node_linker,
        lockfile_only: install.lockfile_only,
        filtered_isolated: setup.shape.filtered_isolated,
        is_hoisted: setup.shape.is_hoisted,
        has_custom_fetcher: setup.chain.custom_fetcher_session.is_some(),
    })
    .then(|| {
        Arc::new(crate::early_materializer::EarlyMaterializer::<Reporter>::new(
            install.config,
            Arc::clone(&owned.tarball_mem_cache),
        ))
    });
    let trust = TrustGate::of(install.config)?;
    let transforms = manifests.transform(
        install.config,
        &owned.catalogs,
        install.lockfile_dir,
        install.deploy_manifest_hook,
    )?;

    let fixed_wanted_lockfile =
        fix_lockfile_copy(&owned.update_seed_policy, install.wanted_lockfile);
    let wanted_lockfile = fixed_wanted_lockfile.as_ref().or(install.wanted_lockfile);
    // The repair copy above replaced the document, so the loader's
    // handle no longer describes `wanted_lockfile`.
    let wanted_lockfile_shared =
        fixed_wanted_lockfile.is_none().then_some(owned.wanted_lockfile_shared.take()).flatten();
    let patches = Patches::resolve(install.config)?;

    // Kept past the resolver hand-off (which consumes `pnpmfile_hook`) so
    // the `afterAllResolved` hook can transform the lockfile before it is
    // written.
    let hooks =
        PnpmfileHooks::load::<Reporter>(setup.chain.pnpmfile_hook.take(), install.lockfile_dir);
    hooks
        .run_pre_resolution::<Reporter>(install.config, install.lockfile_dir, wanted_lockfile)
        .await;
    let reuse = UpdateReuseScopes::settle(
        &owned.update_seed_policy,
        &setup.chain.custom_resolvers,
        wanted_lockfile,
    )
    .await?;
    Ok(ResolutionPrep {
        early_materializer,
        trust,
        transforms,
        fixed_wanted_lockfile,
        wanted_lockfile_shared,
        patches,
        hooks,
        reuse,
    })
}

/// Build the wanted lockfile from the resolved graph and verify a
/// filtered repair against the registry.
///
/// The lockfile verification gate is awaited before the build under
/// `--lockfile-only`, and only for a filtered repair otherwise, so a
/// full install's build overlaps the verification. A materializing
/// install builds the lockfile all the same: the layout and the bin-link
/// pass read its `snapshots:` and `packages:` maps, it costs ~3 ms on the
/// alotta-files fixture, and it is what gets saved anyway.
async fn build_lockfile_phase<'a, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    lockfile_verification_gate: &mut Option<crate::LockfileVerificationGate>,
    resolved_time: BTreeMap<String, String>,
    resolved: &Resolved<'a, Reporter>,
    views: LockfileViews<'_, 'a>,
    verify_filtered_repair: bool,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    let pnpmfile_checksum = pnpmfile_checksum(resolved.after_all_resolved_hook.as_ref()).await;
    if install.lockfile_only {
        await_lockfile_gate(lockfile_verification_gate).await?;
    }
    let phase_start = std::time::Instant::now();
    let built_lockfile = build_lockfile(FreshLockfileBuildOptions {
        inputs: FreshLockfileInputs {
            config: install.config,
            importer_manifests: views.importer_manifests,
            lockfile_specifier_manifests: views.lockfile_specifier_manifests,
            graph: &resolved.merged_graph,
            direct_by_importer: &resolved.direct_by_importer,
            resolved_overrides: resolved.overrides.clone(),
            catalogs: views.catalogs,
            pnpmfile_checksum: pnpmfile_checksum.as_deref(),
            patched_dependency_hashes: resolved.patched_dependency_hashes.as_ref(),
            previous_importers: resolved.guard_previous_importers,
            update_reuse_scope: resolved.guard_update_reuse_scope.clone(),
            update_reuse_scopes_by_importer: resolved.guard_update_reuse_scopes_by_importer.clone(),
            wanted_lockfile: views.wanted_lockfile,
            resolved_time,
        },
        splice: FilteredSplice {
            merge_wanted_lockfile: install.merge_wanted_lockfile,
            real_importer_ids: install.real_importer_ids,
            selected_importer_ids: install.selected_importer_ids,
            lockfile_dir: install.lockfile_dir,
        },
        bumps: SpecBumps {
            manifest_spec_bumps: install.manifest_spec_bumps,
            versions_overrider: resolved.versions_overrider.as_deref(),
        },
    })?;
    if verify_filtered_repair {
        await_lockfile_gate(lockfile_verification_gate).await?;
    }
    verify_repair_if_filtered::<Reporter>(
        verify_filtered_repair,
        &built_lockfile,
        install.resolution_verifiers,
    )
    .await?;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "build_fresh_lockfile",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    Ok(built_lockfile)
}

/// Which importers a selected install materializes, and the lockfile
/// closed over them.
///
/// Built twice: first without a skip set, to give the host probe and the
/// layout the snapshots they plan against, then over the skip set the
/// plan computed. The plan borrows the first closure's lockfile, so the
/// second is a separate value rather than a mutation of the first.
struct MaterializationScope {
    /// `None` when every importer is materialized: the built lockfile
    /// is the closure.
    importer_ids: Option<HashSet<String>>,
    closure: Option<crate::MaterializationClosure>,
}

/// The second closure, with the importers that anchor project links.
struct FinalScope {
    closure: Option<crate::MaterializationClosure>,
    project_anchor_importer_ids: HashSet<String>,
}

impl MaterializationScope {
    fn initial(install: FreshInputs<'_>, is_hoisted: bool, built: &Lockfile) -> Self {
        let importer_ids =
            materialization_importer_ids(install.selected_importer_ids, is_hoisted, built);
        let closure = importer_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                built,
                install.lockfile_dir,
                importer_ids,
                install.included(),
                &SkippedSnapshots::new(),
            )
        });
        MaterializationScope { importer_ids, closure }
    }

    fn lockfile<'l>(&'l self, built: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }

    fn finalize(
        &self,
        install: FreshInputs<'_>,
        is_hoisted: bool,
        built: &Lockfile,
        skipped: &SkippedSnapshots,
    ) -> FinalScope {
        let closure = self.importer_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                built,
                install.lockfile_dir,
                importer_ids,
                install.included(),
                skipped,
            )
        });
        let materialized: HashSet<String> = closure.as_ref().map_or_else(
            || built.importers.keys().cloned().collect(),
            |closure| closure.importer_ids.clone(),
        );
        let project_anchor_importer_ids =
            project_anchor_importer_ids(install.selected_importer_ids, is_hoisted, &materialized);
        FinalScope { closure, project_anchor_importer_ids }
    }
}

impl FinalScope {
    fn lockfile<'l>(&'l self, built: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }
}

/// The lockfiles the materialization plan reads: the one the selected
/// importers materialize, and the full one the skip set's closure walks.
#[derive(Clone, Copy)]
struct PlanLockfiles<'l> {
    initial: &'l Lockfile,
    built: &'l Lockfile,
}

/// What the on-disk phases read as settled. The skip set is the one part
/// they still change, as fetch failures and linking fold into it.
struct FreshPlan<'l> {
    host_node: Option<pnpm_deps_restorer::materialization_plan::HostNode>,
    engine_name: Option<String>,
    deferred_engine_name: Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    layout: VirtualStoreLayout,
    /// Borrows the initial lockfile and the allow-builds policy `run` owns.
    dir_clone_cache: Option<pnpm_deps_restorer::DirCloneCache<'l>>,
    skipped: SkippedSnapshots,
}

/// Detect the host, settle the engine name, build the layout and the
/// directory-clone cache, and compute the skip set. Consumes the early
/// host detection and the node version off `owned`.
async fn plan_fresh_materialization<'l, 'a: 'l, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    probe: HostProbeInputs,
    lockfiles: PlanLockfiles<'l>,
    allow_build_policy: &'l AllowBuildPolicy,
    scope: PlanScope,
) -> Result<FreshPlan<'l>, InstallWithFreshLockfileError> {
    let installability_host = installability_host(
        install.config,
        lockfiles.initial,
        probe.early_host_detection,
        (probe.node_version, install.supported_architectures),
    )
    .await;
    let host_node =
        installability_host.as_ref().map(pnpm_deps_restorer::materialization_plan::HostNode::from);
    let (engine_name, deferred_engine_name) =
        pnpm_deps_restorer::materialization_plan::resolve_engine_name(
            install.config.enable_global_virtual_store,
            lockfiles.initial.snapshots.as_ref(),
            host_node.as_ref(),
        )
        .await;
    let (layout, dir_clone_cache) = lay_out_slots(
        install,
        lockfiles.initial,
        allow_build_policy,
        engine_name.clone(),
        deferred_engine_name.as_ref(),
    );
    let skipped = compute_fresh_skip_set::<Reporter>(
        install,
        lockfiles,
        installability_host.as_ref(),
        scope,
    )?;
    Ok(FreshPlan { host_node, engine_name, deferred_engine_name, layout, dir_clone_cache, skipped })
}

/// Build the slot layout and the directory-clone cache over the lockfile
/// the selected importers materialize.
fn lay_out_slots<'l>(
    install: FreshInputs<'l>,
    initial: &'l Lockfile,
    allow_build_policy: &'l AllowBuildPolicy,
    engine_name: Option<String>,
    deferred_engine_name: Option<&pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
) -> (VirtualStoreLayout, Option<pnpm_deps_restorer::DirCloneCache<'l>>) {
    let phase_start = std::time::Instant::now();
    let layout = VirtualStoreLayout::new(
        install.config,
        install.config.enable_global_virtual_store.then_some(engine_name.as_deref()).flatten(),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.lockfile_dir),
    );
    let dir_clone_cache = pnpm_deps_restorer::DirCloneCache::build(
        install.config,
        install.node_linker,
        engine_name_source(deferred_engine_name, engine_name),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.lockfile_dir),
    );
    log_layout_phase(install.config, phase_start);
    (layout, dir_clone_cache)
}

fn compute_fresh_skip_set<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    lockfiles: PlanLockfiles<'_>,
    installability_host: Option<&pnpm_deps_restorer::InstallabilityHost>,
    scope: PlanScope,
) -> Result<SkippedSnapshots, InstallWithFreshLockfileError> {
    let closure_importer_ids: std::collections::HashSet<String> =
        lockfiles.built.importers.keys().cloned().collect();
    pnpm_deps_restorer::materialization_plan::compute_skip_set::<Reporter>(
        pnpm_deps_restorer::materialization_plan::SkipSetInputs {
            requester: install.requester,
            importers: &lockfiles.initial.importers,
            snapshots: lockfiles.initial.snapshots.as_ref(),
            packages: lockfiles.initial.packages.as_ref(),
            installability_host,
            // The fresh path has just re-resolved the graph, so the
            // previous run's verdicts may no longer hold.
            seed: SkippedSnapshots::new(),
            // Only a full install's `dependency_groups` carries a
            // `--no-optional` intent: a partial run either passes
            // every direct group (`add`, `remove`, `update`) or
            // narrows them for its own reasons (`fetch --dev`,
            // `rebuild`) and must keep its transitive optionals.
            exclude_optional: !scope.include_transitive_optional_dependencies,
            skip_runtimes: install.skip_runtimes,
            closure_lockfile: lockfiles.built,
            closure_root: install.lockfile_dir,
            closure_importer_ids: &closure_importer_ids,
            included: scope.included,
        },
    )
    .map_err(InstallWithFreshLockfileError::Installability)
}

struct HostProbeInputs {
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    node_version: Option<String>,
}

/// Which dependency groups the plan materializes.
#[derive(Clone, Copy)]
struct PlanScope {
    included: IncludedDependencies,
    include_transitive_optional_dependencies: bool,
}

/// The two borrowed views `run` derives from [`Resolved`] once the
/// resolve phase returns.
struct LockfileViews<'v, 'a> {
    importer_manifests: &'v BTreeMap<String, &'a PackageManifest>,
    wanted_lockfile: Option<&'v Lockfile>,
    catalogs: &'v Catalogs,
    lockfile_specifier_manifests: Option<&'v BTreeMap<String, PackageManifest>>,
}

fn is_partial_workspace_selection(
    real_importer_ids: Option<&std::collections::HashSet<String>>,
    selected_importer_ids: Option<&std::collections::HashSet<String>>,
) -> bool {
    matches!(
        (real_importer_ids, selected_importer_ids),
        (Some(real), Some(selected)) if real != selected,
    )
}

fn include_transitive_optional_dependencies(
    is_full_install: bool,
    dependency_groups: &[DependencyGroup],
) -> bool {
    !is_full_install || dependency_groups.contains(&DependencyGroup::Optional)
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

/// Write the freshly-built wanted lockfile to `target`, first running the
/// `afterAllResolved` pnpmfile hook when one is configured.
///
/// `afterAllResolved` receives the resolved lockfile object and
/// returns the (possibly mutated) lockfile that gets written. The round-trip
/// goes through `serde_json::Value` so hook-added keys the typed [`Lockfile`]
/// cannot represent survive to disk; `serde_json`'s `preserve_order` feature
/// keeps the output byte-identical to the typed write when the hook makes no
/// changes. A throwing hook aborts the install.
/// The environment lifecycle scripts run under: `config.extra_env` plus
/// the `NODE_OPTIONS` for the selected project-level dependency loader.
fn build_extra_env(
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> HashMap<String, String> {
    let mut env = config.extra_env.clone();
    if let Some(node_options) = &config.node_options {
        env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    if matches!(node_linker, NodeLinker::Pnp) {
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_require_option(
                &workspace_root.join(crate::PNP_FILENAME),
                node_options,
            ),
        );
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    env
}

/// Whether a custom resolver's `shouldRefreshResolution` hook demands a full
/// re-resolve. A throwing hook propagates and aborts.
async fn custom_resolver_forces_resolve(
    custom_resolvers_raw: &[Arc<dyn pnpm_hooks::CustomResolver>],
    wanted_lockfile: Option<&Lockfile>,
) -> Result<bool, InstallWithFreshLockfileError> {
    let Some(lockfile) = wanted_lockfile else { return Ok(false) };
    crate::check_custom_resolver_force_resolve::check_custom_resolver_force_resolve(
        custom_resolvers_raw,
        lockfile,
    )
    .await
    .map_err(InstallWithFreshLockfileError::CustomResolverForceResolve)
}

/// The hash of the project's `.pnpmfile.{cjs,mjs}` when it exports hooks,
/// `None` otherwise. Resolution has already spawned the pnpmfile worker
/// (every `readPackage` runs through it), so the gate query is cheap.
async fn pnpmfile_checksum(
    after_all_resolved_hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Option<String> {
    let hook = after_all_resolved_hook?;
    hook.calculate_pnpmfile_checksum().await
}

async fn verify_repair_if_filtered<Reporter: pnpm_reporter::Reporter>(
    verify_filtered_repair: bool,
    built_lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<(), InstallWithFreshLockfileError> {
    if !verify_filtered_repair {
        return Ok(());
    }
    verify_merged_repair::<Reporter>(built_lockfile, resolution_verifiers).await
}

/// A lockfile-only resolve never fetches, and a filtered isolated install
/// materializes a subset the prefetcher cannot predict.
fn prefetch_downloads(lockfile_only: bool, filtered_isolated: bool) -> bool {
    !lockfile_only && !filtered_isolated
}

/// What decides whether virtual-store slots may be populated ahead of the
/// lockfile.
#[derive(Clone, Copy)]
struct EarlyMaterializationFit<'a> {
    config: &'a Config,
    node_linker: NodeLinker,
    lockfile_only: bool,
    filtered_isolated: bool,
    is_hoisted: bool,
    has_custom_fetcher: bool,
}

/// Slots can only be populated ahead of the lockfile where their names do not
/// depend on the whole graph (no global virtual store), where the tarballs are
/// prefetched into the cache the materializer waits on, and where the link
/// phase imports straight from the CAS: the macOS directory-clone cache serves
/// project slots from canonical slots it populates itself.
fn early_materialization_eligible(fit: EarlyMaterializationFit<'_>) -> bool {
    !fit.lockfile_only
        && !fit.filtered_isolated
        && !fit.is_hoisted
        && !fit.config.enable_global_virtual_store
        && !pnpm_deps_restorer::DirCloneCache::eligible(fit.config, fit.node_linker)
        && !fit.has_custom_fetcher
}

/// The trust policy the resolver enforces: `Off` means it enforces none.
fn resolver_trust_policy(configured: TrustPolicy) -> Option<TrustPolicy> {
    match configured {
        TrustPolicy::Off => None,
        TrustPolicy::NoDowngrade => Some(TrustPolicy::NoDowngrade),
    }
}

/// The repair copy `pacquet install --fix-lockfile` resolves against, which
/// drops what the repair is meant to rebuild.
fn fix_lockfile_copy(
    update_seed_policy: &UpdateSeedPolicy,
    wanted_lockfile: Option<&Lockfile>,
) -> Option<Lockfile> {
    if !matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile) {
        return None;
    }
    wanted_lockfile.cloned().map(|mut lockfile| {
        lockfile.prepare_for_fix();
        lockfile
    })
}

fn resolver_update_behavior(
    update_seed_policy: &UpdateSeedPolicy,
) -> pnpm_resolving_resolver_base::UpdateBehavior {
    if matches!(update_seed_policy, UpdateSeedPolicy::RefreshRevisions) {
        return pnpm_resolving_resolver_base::UpdateBehavior::Patches;
    }
    pnpm_resolving_resolver_base::UpdateBehavior::Off
}

/// What rules the override fast path out: anything that can rewrite a
/// manifest, or a resolution the rewrite cannot reproduce.
#[derive(Clone, Copy)]
struct FastOverrideFit {
    has_pnpmfile_hook: bool,
    has_custom_resolvers: bool,
    has_patches: bool,
    can_fast_update_overrides: bool,
}

fn fast_override_eligible(fit: FastOverrideFit) -> bool {
    !fit.has_pnpmfile_hook
        && !fit.has_custom_resolvers
        && !fit.has_patches
        && fit.can_fast_update_overrides
}

/// A dry run never prompts and never persists what a prompt would have
/// settled.
fn interactive_policy(
    can_prompt: bool,
    persist_policy_excludes: bool,
    dry_run: bool,
) -> (bool, bool) {
    (can_prompt && !dry_run, persist_policy_excludes && !dry_run)
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

/// The concurrent pre-resolve verification of the existing lockfile must have
/// its verdict before anything sensitive: the symlink / bin-link phases, the
/// dependency builds, and the lockfile save all run on a trusted lockfile
/// only.
async fn await_lockfile_gate(
    gate: &mut Option<crate::install::LockfileVerificationGate>,
) -> Result<(), InstallWithFreshLockfileError> {
    let Some(gate) = gate.take() else { return Ok(()) };
    gate.wait().await.map_err(InstallWithFreshLockfileError::LockfileVerification)
}

/// What the on-disk phases read once the lockfile is built and the
/// materialization plan is fixed.
struct OnDiskInputs<'a> {
    ctx: &'a pnpm_deps_restorer::InstallContext<'a>,
    http_client: &'a ThrottledClient,
    prune_orphans: bool,
    include_transitive_optional_dependencies: bool,
    supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    current_lockfile: Option<&'a Lockfile>,
    prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    tarball_mem_cache: &'a Arc<MemCache>,
    materialization_lockfile: &'a Lockfile,
    importer_manifests: &'a std::collections::BTreeMap<String, &'a PackageManifest>,
    dependency_groups: &'a [DependencyGroup],
    project_anchor_importer_ids: &'a std::collections::HashSet<String>,
    dir_clone_cache: Option<&'a pnpm_deps_restorer::DirCloneCache<'a>>,
    host_node: Option<&'a pnpm_deps_restorer::materialization_plan::HostNode>,
    engine_name: Option<String>,
    deferred_engine_name: Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    patched_dependencies: Option<&'a pnpm_patching::PatchGroupRecord>,
    custom_fetcher_session: Option<&'a Arc<pnpm_deps_restorer::CustomFetcherSession>>,
    store_index_ref: Option<&'a pnpm_store_dir::SharedReadonlyStoreIndex>,
    store_index_writer: Arc<pnpm_store_dir::StoreIndexWriter>,
    caches: &'a resolver_setup::StoreCaches,
}

/// What the on-disk phases leave for the install's result.
struct OnDiskOutput {
    hoisted_dependencies: crate::HoistedDependencies,
    hoisted_locations: std::collections::BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    ignored_builds: Vec<String>,
    deferred_builds: Vec<String>,
    skipped: SkippedSnapshots,
}

impl<'a> OnDiskInputs<'a> {
    /// See `linking::run_link_phase` for why this anchors on
    /// `modules_dir.parent()` rather than the install root.
    fn symlink_root(&self) -> &'a Path {
        self.ctx.config.modules_dir.parent().unwrap_or(self.ctx.workspace_root)
    }

    /// Materialize the virtual store. Skipped snapshots stay out of every
    /// map the output carries; the fetch failures it reports are folded into
    /// the skip set by the caller before anything links.
    async fn materialize<Reporter: self::Reporter + 'static>(
        &self,
        skipped: &SkippedSnapshots,
    ) -> Result<CreateVirtualStoreOutput, InstallWithFreshLockfileError> {
        let phase_start = std::time::Instant::now();
        let materialized = CreateVirtualStore {
            ctx: self.ctx,
            http_client: self.http_client,
            entries: self.materialization_lockfile.into(),
            current_entries: LockfileEntries::of_previous_install(
                self.current_lockfile,
                self.ctx.config.force,
            ),
            store_index_writer: &self.store_index_writer,
            store_context: Some(pnpm_deps_restorer::CreateVirtualStoreStoreContext {
                index: self.store_index_ref,
                verified_files_cache: &self.caches.verified_files,
            }),
            cas_prefetch: None,
            skipped,
            include_optional_dependencies: self.include_transitive_optional_dependencies,
            supported_architectures: self.supported_architectures,
            dir_clone_cache: self.dir_clone_cache,
            progress_reported: &self.caches.progress_reported,
            // Share the resolve-time prefetcher's in-flight downloads with
            // the cold batch. The `PrefetchingResolver` streams each
            // tarball into `tarball_mem_cache` keyed by URL; the cold
            // batch's only on-disk dedup is the store-index row, which the
            // prefetcher's writer commits asynchronously. Without the
            // shared cache a snapshot whose prefetch hasn't committed its
            // row yet is classified cold and re-downloaded — a race that
            // routing the cold batch through the mem cache fixes by
            // reusing the in-flight download instead.
            tarball_mem_cache: Some(self.tarball_mem_cache),
            custom_fetcher_session: self.custom_fetcher_session,
            // The fresh path's concurrent gate verifies the *previous*
            // lockfile while this run fetches the new graph; the two
            // entry sets differ, so no fetch plan is published and the
            // verifier keeps its metadata-backed path.
            planned_canonical_fetches: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallWithFreshLockfileError::CreateVirtualStore)?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "create_virtual_store",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );
        Ok(materialized)
    }

    /// Link the materialized store into every project and report
    /// `importing_done`, which reporters use to close the import progress
    /// display before the `pnpm:lifecycle` events of the build.
    fn link<Reporter: self::Reporter + 'static>(
        &self,
        materialized: &mut CreateVirtualStoreOutput,
        skipped: &mut SkippedSnapshots,
    ) -> Result<pnpm_deps_restorer::linking::LinkPhaseOutput, InstallWithFreshLockfileError> {
        let project_manifests: Vec<(std::path::PathBuf, &PackageManifest)> = self
            .importer_manifests
            .iter()
            .filter(|(id, _)| self.project_anchor_importer_ids.contains(id.as_str()))
            .map(|(id, manifest)| (self.ctx.workspace_root.join(id), *manifest))
            .collect();
        let package_map_project_manifests: Vec<(std::path::PathBuf, &PackageManifest)> = self
            .importer_manifests
            .iter()
            .map(|(id, manifest)| (self.ctx.workspace_root.join(id), *manifest))
            .collect();
        let root_component_importers: std::collections::HashSet<String> = self
            .importer_manifests
            .iter()
            .filter(|(id, _)| self.project_anchor_importer_ids.contains(id.as_str()))
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits()
                    == Some(pnpm_deps_restorer::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(id, _)| id.clone())
            .collect();

        let linked = pnpm_deps_restorer::linking::run_link_phase::<Reporter>(
            pnpm_deps_restorer::linking::LinkPhaseInputs {
                ctx: self.ctx,
                requires_build_by_snapshot: None,
                symlink_root: self.symlink_root(),
                trusted_importer_ids: self.project_anchor_importer_ids,
                root_component_importers: &root_component_importers,
                sidecar_lockfile: self.materialization_lockfile,
                lockfile: self.materialization_lockfile,
                current_lockfile: self.current_lockfile,
                materialized_snapshots: Some(&materialized.materialized_snapshots),
                project_manifests: &project_manifests,
                package_map_project_manifests: &package_map_project_manifests,
                dependency_groups: self.dependency_groups,
                package_manifests: &materialized.package_manifests,
                cas_paths_by_pkg_id: materialized.cas_paths_by_pkg_id.take(),
                prune_orphans: self.prune_orphans,
                prior_hoisted_dependencies: self.prior_hoisted_dependencies,
                host_node: self.host_node,
                supported_architectures: self.supported_architectures,
            },
            skipped,
        )
        .map_err(InstallWithFreshLockfileError::LinkPhase)?;
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: self.ctx.requester.to_string(),
            stage: Stage::ImportingDone,
        }));
        Ok(linked)
    }

    /// Run lifecycle scripts, report ignored builds, and re-link top-level
    /// bins: the build phase the frozen path runs, so `pacquet add esbuild`
    /// reports the blocked `esbuild` build (and builds approved packages)
    /// exactly like `pnpm`. Scripts see the install root as `INIT_CWD`; the
    /// post-build bin link anchors on [`Self::symlink_root`], where this
    /// path placed `node_modules`.
    ///
    /// Consumes the inputs: the store-index writer handle is dropped once
    /// the side-effects-cache rows are queued, so the channel closes and
    /// the writer task starts winding down while the caller finishes. The
    /// install driver awaits it as `store_index_teardown`.
    async fn build<Reporter: self::Reporter + 'static>(
        self,
        materialized: &CreateVirtualStoreOutput,
        linked: &pnpm_deps_restorer::linking::LinkPhaseOutput,
        skipped: &SkippedSnapshots,
    ) -> Result<crate::BuildModulesOutput, InstallWithFreshLockfileError> {
        // Resolve the deferred `node --version` probe (non-GVS path); it
        // overlapped `CreateVirtualStore`. Falls back to the synchronous
        // value when the probe wasn't deferred.
        let top_level_bin_root = self.symlink_root();
        let engine_name = settle_engine_name(self.deferred_engine_name, self.engine_name).await;
        let extra_env =
            build_extra_env(self.ctx.config, self.ctx.node_linker, self.ctx.workspace_root);
        publish_deps_requiring_build(
            self.deps_requiring_build_sink.as_ref(),
            &materialized.requires_build_by_snapshot,
        );
        let built = crate::install_frozen_lockfile::run_build_phase::<Reporter>(
            &crate::install_frozen_lockfile::BuildPhaseInputs {
                config: self.ctx.config,
                workspace_root: self.ctx.workspace_root,
                top_level_bin_root,
                layout: self.ctx.layout,
                snapshots: self.materialization_lockfile.snapshots.as_ref(),
                packages: self.materialization_lockfile.packages.as_ref(),
                importers: &self.materialization_lockfile.importers,
                dependency_groups: self.dependency_groups,
                // Reuse the record resolved earlier for the resolver so the
                // patch files aren't hashed a second time.
                patch_groups: self.patched_dependencies,
                allow_build_policy: self.ctx.allow_build_policy,
                side_effects_maps_by_snapshot: &materialized.side_effects_maps_by_snapshot,
                requires_build_by_snapshot: &materialized.requires_build_by_snapshot,
                materialized_snapshots: &materialized.materialized_snapshots,
                engine_name: engine_name.as_deref(),
                extra_env: &extra_env,
                store_index_writer: &self.store_index_writer,
                skipped,
                hoisted_pkg_roots_by_key: linked.hoisted_pkg_roots_by_key.as_ref(),
                is_hoisted: self.ctx.is_hoisted(),
                publicly_hoisted_for_post_build: &linked.publicly_hoisted_for_post_build,
                logged_methods: self.ctx.logged_methods,
                // The fresh-resolve path never serves an explicit
                // `pacquet rebuild`; rebuilds always take the frozen path.
                rebuild: None,
                link_options: self.ctx.link_options,
            },
        )
        .map_err(InstallWithFreshLockfileError::BuildPhase)?;
        drop(self.store_index_writer);
        Ok(built)
    }
}

/// Materialize the virtual store, link it into every project, and run the
/// dependency builds. The lockfile is persisted by the caller, after this
/// returns, so a partial install cannot leave one behind.
async fn run_on_disk_phases<Reporter: self::Reporter + 'static>(
    inputs: OnDiskInputs<'_>,
    skipped: &mut SkippedSnapshots,
    lockfile_verification_gate: &mut Option<crate::LockfileVerificationGate>,
) -> Result<OnDiskOutput, InstallWithFreshLockfileError> {
    let ctx = inputs.ctx;
    let lockfile = inputs.materialization_lockfile;
    let mut materialized = inputs.materialize::<Reporter>(skipped).await?;

    // The concurrent pre-resolve verification of the existing
    // lockfile must have its verdict before anything sensitive: the
    // symlink / bin-link phases, the dependency builds, and the
    // lockfile save below all run on a trusted lockfile only.
    await_lockfile_gate(lockfile_verification_gate).await?;
    fold_fetch_failures(skipped, std::mem::take(&mut materialized.fetch_failed));

    let linked = inputs.link::<Reporter>(&mut materialized, skipped)?;
    let crate::BuildModulesOutput { ignored_builds, deferred_builds, mutated_slots: _ } =
        inputs.build::<Reporter>(&materialized, &linked, skipped).await?;

    let injected_deps = crate::collect_injected_deps(
        ctx.layout,
        ctx.workspace_root,
        lockfile.into(),
        skipped,
        ctx.is_hoisted().then_some(&linked.hoisted_locations),
    );
    Ok(OnDiskOutput {
        hoisted_dependencies: linked.hoisted_dependencies,
        hoisted_locations: linked.hoisted_locations,
        injected_deps,
        ignored_builds,
        deferred_builds,
        skipped: std::mem::take(skipped),
    })
}

/// The importers a selected install materializes. A hoisted linker shares one
/// tree, so it still materializes every importer.
fn materialization_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    is_hoisted: bool,
    built_lockfile: &Lockfile,
) -> Option<HashSet<String>> {
    let selected_importer_ids = selected_importer_ids?;
    if is_hoisted {
        return Some(built_lockfile.importers.keys().cloned().collect());
    }
    Some(selected_importer_ids.clone())
}

/// The host the installability checks run against, resolved from the
/// overlapped probe when one was started.
async fn installability_host(
    config: &Config,
    lockfile: &Lockfile,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    host: (Option<String>, Option<&pnpm_package_is_installable::SupportedArchitectures>),
) -> Option<pnpm_deps_restorer::InstallabilityHost> {
    let (node_version, supported_architectures) = host;
    let needed = !config.force
        && lockfile.packages.as_ref().is_some_and(|packages| {
            lockfile
                .snapshots
                .as_ref()
                .is_some_and(|snapshots| crate::any_installability_constraint(snapshots, packages))
        });
    match (early_host_detection, needed) {
        (Some(detection), true) => detection.resolve().await,
        (_, needed) => {
            pnpm_deps_restorer::materialization_plan::detect_installability_host(
                needed,
                config.engine_strict,
                node_version,
                supported_architectures,
            )
            .await
        }
    }
}

fn engine_name_source(
    deferred: Option<&pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    engine_name: Option<String>,
) -> pnpm_deps_restorer::EngineNameSource {
    match deferred {
        Some(deferred) => pnpm_deps_restorer::EngineNameSource::Pending(deferred.shared()),
        None => pnpm_deps_restorer::EngineNameSource::Ready(engine_name),
    }
}

/// Only a global virtual store makes the layout build cost anything worth
/// timing.
fn log_layout_phase(config: &Config, phase_start: std::time::Instant) {
    if !config.enable_global_virtual_store {
        return;
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "virtual_store_layout_new",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
}

/// The importers whose own project manifests anchor the link phase.
fn project_anchor_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    is_hoisted: bool,
    materialization_importer_ids: &HashSet<String>,
) -> HashSet<String> {
    match selected_importer_ids {
        Some(selected_importer_ids) if is_hoisted => selected_importer_ids.clone(),
        _ => materialization_importer_ids.clone(),
    }
}

async fn finish_early_materialization<Reporter: self::Reporter + 'static>(
    materializer: Option<&crate::early_materializer::EarlyMaterializer<Reporter>>,
    wanted: Option<&HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>>,
    skipped: &SkippedSnapshots,
    logged_methods: &AtomicU8,
) {
    let Some(materializer) = materializer else { return };
    let phase_start = std::time::Instant::now();
    let materialized = materializer
        .finish(
            |key| {
                wanted.is_some_and(|snapshots| snapshots.contains_key(key))
                    && !skipped.contains(key)
            },
            logged_methods,
        )
        .await;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "early_materialization",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        materialized,
        "phase complete",
    );
}

fn fold_fetch_failures(
    skipped: &mut SkippedSnapshots,
    fetch_failed: HashSet<pnpm_lockfile::PackageKey>,
) {
    for key in fetch_failed {
        skipped.add_fetch_failed(key);
    }
}

/// `CreateVirtualStore` keeps skipped snapshots out of this map, so it holds
/// only what the install put on disk. See [`crate::DepsRequiringBuildSink`].
fn publish_deps_requiring_build(
    sink: Option<&crate::DepsRequiringBuildSink>,
    requires_build_by_snapshot: &HashMap<pnpm_lockfile::PackageKey, bool>,
) {
    let Some(sink) = sink else { return };
    let deps_requiring_build = requires_build_by_snapshot
        .iter()
        .filter(|(_, requires_build)| **requires_build)
        .map(|(snapshot_key, _)| snapshot_key.to_string())
        .collect();
    *sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(deps_requiring_build);
}

/// Resolve the deferred `node --version` probe (non-GVS path); it overlapped
/// `CreateVirtualStore`. Falls back to the synchronous value when the probe
/// wasn't deferred.
async fn settle_engine_name(
    deferred: Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    engine_name: Option<String>,
) -> Option<String> {
    match deferred {
        Some(deferred) => deferred.handle.await.ok().flatten(),
        None => engine_name,
    }
}

/// The built wanted lockfile whenever lockfiles are enabled, whether or
/// not this run wrote it, and whether a verification may be recorded
/// against the file on disk, which only a written one allows.
struct PersistedLockfile {
    lockfile: Option<Lockfile>,
    can_record_lockfile_verification: bool,
}

/// Save `pnpm-lock.yaml` after the build phase succeeds, so a partial install
/// can't leave a lockfile pointing at slots that never landed on disk.
async fn persist_fresh_lockfile(
    built_lockfile: Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    save_lockfile: bool,
    after_all_resolved: (Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>, Option<pnpm_hooks::LogFn>),
) -> Result<PersistedLockfile, InstallWithFreshLockfileError> {
    if !config.lockfile {
        return Ok(PersistedLockfile { lockfile: None, can_record_lockfile_verification: false });
    }
    if !save_lockfile {
        // Nothing was persisted, so there is no `pnpm-lock.yaml` whose
        // verification a later install could key off.
        return Ok(PersistedLockfile {
            lockfile: Some(built_lockfile),
            can_record_lockfile_verification: false,
        });
    }
    let (hook, log) = after_all_resolved;
    let target = lockfile_dir.join(config.wanted_lockfile_name());
    let can_record_lockfile_verification =
        save_wanted_lockfile(&built_lockfile, &target, hook, log).await?;
    Ok(PersistedLockfile { lockfile: Some(built_lockfile), can_record_lockfile_verification })
}

/// Importers whose linked workspace dependency declares
/// `peerDependencies`.
///
/// The lockfile walk that renders the report reads a linked project's
/// manifest and checks its peers against the *consuming* importer's
/// dependencies. Peer resolution has no counterpart for that check — a
/// `link:` node's own peers are the linked importer's business — so
/// these importers never reach
/// `peer_dependency_issues_by_importer` and have to join the report's
/// candidate set on their own. Only the direct consumer is needed: it
/// is the importer whose dependencies the check compares against.
///
/// Answered from the manifests the install already parsed, so a
/// workspace whose projects declare no peers costs one pass over the
/// declared dependencies and no I/O. Over-approximates — a
/// `workspace:` dependency the lockfile records as an injected
/// directory rather than a link still counts, as does a target this
/// cannot name — which only widens the walk.
fn importers_consuming_linked_peers(
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    lockfile_dir: &Path,
) -> HashSet<String> {
    let declares_peers = |manifest: &PackageManifest| {
        manifest
            .value()
            .get("peerDependencies")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|peers| !peers.is_empty())
    };
    fn project_name(manifest: &PackageManifest) -> Option<&str> {
        manifest.value().get("name")?.as_str()
    }
    let scan = LinkedPeerScan {
        importer_manifests,
        lockfile_dir,
        peer_declaring_ids: importer_manifests
            .iter()
            .filter(|(_, manifest)| declares_peers(manifest))
            .map(|(importer_id, _)| importer_id.as_str())
            .collect(),
        peer_declaring_names: importer_manifests
            .values()
            .filter(|manifest| declares_peers(manifest))
            .filter_map(|manifest| project_name(manifest))
            .collect(),
        project_names: importer_manifests
            .values()
            .filter_map(|manifest| project_name(manifest))
            .collect(),
    };

    let mut consumers = HashSet::new();
    for (importer_id, manifest) in importer_manifests {
        let importer_dir = lockfile_dir.join(importer_id);
        let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        let consumes = manifest.dependencies(groups).any(|(entry_key, bare_specifier)| {
            linked_target_may_declare_peers(&scan, &importer_dir, entry_key, bare_specifier)
        });
        if consumes {
            consumers.insert(importer_id.clone());
        }
    }
    consumers
}

/// The workspace projects a link target is matched against.
struct LinkedPeerScan<'a> {
    importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    lockfile_dir: &'a Path,
    peer_declaring_ids: HashSet<&'a str>,
    peer_declaring_names: HashSet<&'a str>,
    project_names: HashSet<&'a str>,
}

/// Whether one declared dependency links to a project that may declare peers.
///
/// A target whose peers are unknown here counts. The walk reads such a
/// manifest when it resolves inside the lockfile directory and skips one that
/// escapes; symlinks decide which, so both count.
fn linked_target_may_declare_peers(
    scan: &LinkedPeerScan<'_>,
    importer_dir: &Path,
    entry_key: &str,
    bare_specifier: &str,
) -> bool {
    if let Some(spec) = pnpm_workspace_spec::WorkspaceSpec::parse(bare_specifier) {
        // `workspace:<name>@<range>` names the project it links
        // to; the bare form takes that name from the entry key.
        // The range picks among the projects sharing that name,
        // so any one of them declaring a peer counts — reaching
        // for the picked version would duplicate
        // `resolve_workspace_range` to narrow an answer that is
        // only ever "walk this importer too".
        let linked_name = spec.alias.as_deref().unwrap_or(entry_key);
        return scan.peer_declaring_names.contains(linked_name)
            || !scan.project_names.contains(linked_name);
    }
    // Only `file:` reads the name: it resolves to a package
    // when that names a tarball, and only its directory form
    // becomes the `link:` entry the walk inspects. A `link:`
    // is a directory whatever it is called.
    let Some(relative) = bare_specifier.strip_prefix("link:").or_else(|| {
        bare_specifier
            .strip_prefix("file:")
            .filter(|_| !pnpm_resolving_local_resolver::is_tarball_filename(bare_specifier))
    }) else {
        return false;
    };
    let linked_id =
        pnpm_workspace::importer_id_from_root_dir(scan.lockfile_dir, &importer_dir.join(relative));
    scan.peer_declaring_ids.contains(linked_id.as_str())
        || !scan.importer_manifests.contains_key(&linked_id)
}

struct LockfileOnlyOptions<'a> {
    built_lockfile: Lockfile,
    peer_issue_importer_ids: HashSet<String>,
    config: &'a Config,
    lockfile_dir: &'a Path,
    requester: &'a str,
    dry_run: bool,
    save_lockfile: bool,
    after_all_resolved_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    after_all_resolved_log: Option<pnpm_hooks::LogFn>,
    store_index_writer: Arc<pnpm_store_dir::StoreIndexWriter>,
    writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}

async fn verify_merged_repair<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<(), InstallWithFreshLockfileError> {
    pnpm_lockfile_verification::verify_lockfile_resolutions::<Reporter>(
        lockfile,
        resolution_verifiers,
        &pnpm_lockfile_verification::VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .map_err(InstallWithFreshLockfileError::LockfileVerification)
}

/// Tail of the `--lockfile-only` path: persist the freshly-built
/// lockfile, close the store-index writer, and report the install done.
///
/// `--dry-run` builds the would-be lockfile so the caller can diff it,
/// but never persists it. A plain `--lockfile-only` writes it (unless
/// `lockfile: false`). Nothing was materialized, so no build phase ran
/// and nothing was ignored, deferred, or skipped.
async fn finish_lockfile_only<Reporter: self::Reporter>(
    opts: LockfileOnlyOptions<'_>,
) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
    let (wanted_lockfile, can_record_lockfile_verification) = if opts.dry_run || !opts.save_lockfile
    {
        (Some(opts.built_lockfile), false)
    } else if opts.config.lockfile {
        let can_record_lockfile_verification = save_wanted_lockfile(
            &opts.built_lockfile,
            &opts.lockfile_dir.join(opts.config.wanted_lockfile_name()),
            opts.after_all_resolved_hook,
            opts.after_all_resolved_log,
        )
        .await?;
        (Some(opts.built_lockfile), can_record_lockfile_verification)
    } else {
        (None, false)
    };

    // Close the writer cleanly even though no rows were written,
    // mirroring the materializing path: drop closes the channel, the
    // caller awaits the returned task after its own tail writes.
    drop(opts.store_index_writer);

    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: opts.requester.to_string(),
        stage: Stage::ImportingDone,
    }));
    Ok(InstallWithFreshLockfileResult {
        hoisted_dependencies: HoistedDependencies::new(),
        hoisted_locations: BTreeMap::new(),
        injected_deps: BTreeMap::new(),
        peer_issue_importer_ids: opts.peer_issue_importer_ids,
        wanted_lockfile,
        can_record_lockfile_verification,
        ignored_builds: Vec::new(),
        deferred_builds: Vec::new(),
        skipped: SkippedSnapshots::new(),
        store_index_teardown: opts.writer_task,
    })
}

async fn save_wanted_lockfile(
    built_lockfile: &Lockfile,
    target: &Path,
    hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    log: Option<pnpm_hooks::LogFn>,
) -> Result<bool, InstallWithFreshLockfileError> {
    let Some(hook) = hook else {
        built_lockfile
            .save_to_path(target)
            .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
        return Ok(true);
    };

    let value = serde_json::to_value(built_lockfile)
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedSerialize)?;
    let ctx = pnpm_hooks::HookContext { log: log.unwrap_or_else(|| Arc::new(|_| {})), dir: None };
    let result = hook
        .after_all_resolved(value, ctx)
        .await
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedHook)?;

    // `Null` means the pnpmfile has no `afterAllResolved` hook, so write the
    // typed lockfile unchanged.
    if result.is_null() {
        built_lockfile.save_to_path(target)
    } else {
        pnpm_lockfile::save_value_to_path(&result, target)
    }
    .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
    Ok(result.is_null())
}

fn parse_config_overrides(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<Option<Vec<pnpm_config_parse_overrides::VersionOverride>>, InstallWithFreshLockfileError>
{
    match config.overrides.as_ref() {
        Some(map) if !map.is_empty() => {
            pnpm_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)
                .map(Some)
                .map_err(InstallWithFreshLockfileError::InvalidOverrides)
        }
        _ => Ok(None),
    }
}

fn resolved_overrides_map(
    parsed: &[pnpm_config_parse_overrides::VersionOverride],
) -> IndexMap<String, String> {
    parsed.iter().map(|entry| (entry.selector.clone(), entry.new_bare_specifier.clone())).collect()
}

fn overrides_match(
    lockfile: Option<&IndexMap<String, String>>,
    config: Option<&IndexMap<String, String>>,
) -> bool {
    let lockfile = lockfile.filter(|map| !map.is_empty());
    let config = config.filter(|map| !map.is_empty());
    match (lockfile, config) {
        (None, None) => true,
        (Some(lockfile), Some(config)) => {
            lockfile.len() == config.len()
                && lockfile.iter().all(|(key, value)| {
                    config.get(key).is_some_and(|config_value| config_value == value)
                })
        }
        _ => false,
    }
}

fn ignored_optional_dependencies_match(left: Option<&[String]>, right: Option<&[String]>) -> bool {
    let left: HashSet<_> = left.unwrap_or_default().iter().collect();
    let right: HashSet<_> = right.unwrap_or_default().iter().collect();
    left == right
}

fn compose_manifest_hooks(
    first: Option<ManifestHook>,
    second: Option<ManifestHook>,
) -> Option<ManifestHook> {
    match (first, second) {
        (None, None) => None,
        (Some(hook), None) | (None, Some(hook)) => Some(hook),
        (Some(first), Some(second)) => {
            Some(Arc::new(move |manifest| second(first(manifest))) as ManifestHook)
        }
    }
}

/// Build the [`Lockfile`] for `<lockfile_dir>/pnpm-lock.yaml`: the merged
/// resolver graph and the per-importer direct-deps maps lifted to the
/// wire shape, spliced back over the importers a filtered install did
/// not resolve, with the manifest spec bumps applied.
struct FreshLockfileBuildOptions<'a> {
    inputs: FreshLockfileInputs<'a>,
    splice: FilteredSplice<'a>,
    bumps: SpecBumps<'a>,
}

/// What [`dependencies_graph_to_lockfile()`] lifts to the wire shape.
struct FreshLockfileInputs<'a> {
    config: &'a Config,
    importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    lockfile_specifier_manifests: Option<&'a BTreeMap<String, PackageManifest>>,
    graph: &'a pnpm_resolving_deps_resolver::DependenciesGraph,
    direct_by_importer:
        &'a BTreeMap<String, BTreeMap<String, pnpm_resolving_deps_resolver::DepPath>>,
    resolved_overrides: Option<IndexMap<String, String>>,
    catalogs: &'a pnpm_catalogs_types::Catalogs,
    pnpmfile_checksum: Option<&'a str>,
    patched_dependency_hashes: Option<&'a BTreeMap<String, String>>,
    /// The previous run's lockfile importer entries, threaded into the
    /// pnpm/pnpm#10433 guard so an untouched workspace dependency keeps
    /// its prior `link:` entry. `None` on a first install.
    previous_importers: Option<&'a HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    /// How this install reuses the prior resolution (from the `pacquet
    /// update` seed policy), also consumed by the pnpm/pnpm#10433 guard.
    update_reuse_scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    /// Per-importer update scopes (the `ByImporter` policy of a recursive
    /// update), so the guard honors `pacquet update <name> --recursive`
    /// targeting per importer rather than the workspace-wide default.
    update_reuse_scopes_by_importer:
        BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
    /// The previous run's lockfile: its `packages:` seed the reuse and its
    /// `time:` is layered under this run's. `None` on a first install.
    wanted_lockfile: Option<&'a Lockfile>,
    /// Publish dates this run resolved for the direct dependencies,
    /// layered over the ones [`Self::wanted_lockfile`] recorded. Empty
    /// unless the install resolved `time-based`.
    resolved_time: BTreeMap<String, String>,
}

/// The previous run's lockfile, spliced back over the importers a
/// filtered install did not resolve.
struct FilteredSplice<'a> {
    /// Intact prior lockfile used when splicing back unselected importers.
    merge_wanted_lockfile: Option<&'a Lockfile>,
    /// Every importer the workspace declares, and the subset this run
    /// resolved. Both `Some` and unequal means the install is filtered,
    /// so the unselected importers keep their previous entries.
    real_importer_ids: Option<&'a std::collections::HashSet<String>>,
    selected_importer_ids: Option<&'a std::collections::HashSet<String>>,
    lockfile_dir: &'a Path,
}

impl FilteredSplice<'_> {
    fn apply(self, freshly_resolved: Lockfile) -> Result<Lockfile, InstallWithFreshLockfileError> {
        match (self.real_importer_ids, self.selected_importer_ids) {
            (Some(real_importer_ids), Some(selected_importer_ids)) => {
                crate::merge_filtered_wanted_lockfile(
                    self.merge_wanted_lockfile,
                    freshly_resolved,
                    real_importer_ids,
                    selected_importer_ids,
                    self.lockfile_dir,
                )
                .map_err(InstallWithFreshLockfileError::MergeFilteredWantedLockfile)
            }
            _ => Ok(freshly_resolved),
        }
    }
}

/// See [`InstallWithFreshLockfile::manifest_spec_bumps`].
struct SpecBumps<'a> {
    manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    /// The override set the run resolved under. Consulted only alongside
    /// [`Self::manifest_spec_bumps`], to leave a declaration an override
    /// governs where the project wrote it.
    versions_overrider: Option<&'a crate::VersionsOverrider>,
}

impl SpecBumps<'_> {
    fn apply(self, built: &mut Lockfile, importer_manifests: &BTreeMap<String, &PackageManifest>) {
        let Some(bumps) = self.manifest_spec_bumps else { return };
        let overridden =
            self.versions_overrider.filter(|overrider| !overrider.is_empty()).map(|overrider| {
                crate::manifest_spec_bumps::OverriddenDeclarations { overrider, importer_manifests }
            });
        crate::manifest_spec_bumps::apply_manifest_spec_bumps(built, bumps, overridden.as_ref());
    }
}

fn build_lockfile(
    opts: FreshLockfileBuildOptions<'_>,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    let FreshLockfileBuildOptions { inputs, splice, bumps } = opts;
    let importer_manifests = inputs.importer_manifests;
    let freshly_resolved = build_fresh_lockfile(inputs).map_err(|error| {
        InstallWithFreshLockfileError::DependenciesGraphToLockfile(Box::new(error))
    })?;
    let mut built = splice.apply(freshly_resolved)?;
    bumps.apply(&mut built, importer_manifests);
    Ok(built)
}

fn build_fresh_lockfile(
    inputs: FreshLockfileInputs<'_>,
) -> Result<Lockfile, DependenciesGraphToLockfileError> {
    let mut importers = BTreeMap::new();
    for (id, manifest) in inputs.importer_manifests {
        let direct = inputs.direct_by_importer.get(id).cloned().unwrap_or_default();
        let manifest = inputs
            .lockfile_specifier_manifests
            .and_then(|manifests| manifests.get(id))
            .unwrap_or(*manifest);
        importers.insert(
            id.clone(),
            ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct },
        );
    }
    let registries_by_prefix = registries_by_prefix(inputs.config);
    let config = inputs.config;
    dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: inputs.graph,
        registry_options_by_url: &config.registry_options_by_url,
        auto_install_peers: config.auto_install_peers,
        dedupe_peers: config.dedupe_peers,
        exclude_links_from_lockfile: config.exclude_links_from_lockfile,
        inject_workspace_packages: config.inject_workspace_packages,
        peers_suffix_max_length: (config.peers_suffix_max_length
            != pnpm_config::default_peers_suffix_max_length())
        .then_some(config.peers_suffix_max_length),
        overrides: inputs.resolved_overrides,
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        patched_dependencies: inputs.patched_dependency_hashes.cloned(),
        package_extensions_checksum: compute_package_extensions_checksum(config),
        pnpmfile_checksum: inputs.pnpmfile_checksum.map(str::to_string),
        catalogs: inputs.catalogs,
        registry: &config.registry,
        registries_by_prefix: &registries_by_prefix,
        lockfile_include_tarball_url: config.lockfile_include_tarball_url,
        previous_importers: inputs.previous_importers,
        previous_packages: inputs.wanted_lockfile.and_then(|lockfile| lockfile.packages.as_ref()),
        update_reuse_scope: inputs.update_reuse_scope,
        update_reuse_scopes_by_importer: inputs.update_reuse_scopes_by_importer,
        time: merge_recorded_time(inputs.wanted_lockfile, inputs.resolved_time),
    })
}

/// Same merge the resolver chain performs; the config was already
/// validated at resolver construction, so skip re-validation here.
fn registries_by_prefix(config: &Config) -> HashMap<String, String> {
    pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX
        .iter()
        .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
        .chain(config.registries_by_prefix.iter().map(|(name, url)| (name.clone(), url.clone())))
        .collect()
}

/// The `time:` section the rewritten lockfile carries: what the prior
/// lockfile recorded, with this run's freshly resolved publish dates
/// layered over it. Keeping the prior entries is what preserves a
/// recorded date for a dependency whose packument does not carry one;
/// saving prunes whatever is no longer a direct dependency.
fn merge_recorded_time(
    wanted_lockfile: Option<&Lockfile>,
    resolved_time: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let Some(recorded) = wanted_lockfile.and_then(|lockfile| lockfile.time.as_ref()) else {
        return resolved_time;
    };
    let mut time = recorded.clone();
    time.extend(resolved_time);
    time
}

pub(crate) fn compute_package_extensions_checksum(config: &Config) -> Option<String> {
    let extensions =
        config.package_extensions.as_ref().filter(|extensions| !extensions.is_empty())?;
    let value = serde_json::to_value(extensions).ok()?;
    pnpm_graph_hasher::hash_object_nullable_with_prefix(&value)
}

#[cfg(test)]
mod tests;

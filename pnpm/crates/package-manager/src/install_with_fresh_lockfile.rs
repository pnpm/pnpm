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
use pnpm_lockfile::{Lockfile, SaveLockfileError};
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
use pnpm_store_dir::SharedVerifiedFilesCache;
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
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
pub struct InstallWithFreshLockfile<'a, DependencyGroupList> {
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
    pub dependency_groups: DependencyGroupList,
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
    /// Intact prior lockfile used to restore unselected projects after a
    /// filtered repair resolves against a sanitized seed.
    pub merge_wanted_lockfile: Option<&'a Lockfile>,
    /// Effective `nodeVersion`: an explicit config value, otherwise the
    /// minimum version declared by the root manifest's runtime engine.
    pub node_version: Option<String>,
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
    #[display("{_0}")]
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
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    AfterAllResolvedHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The freshly-built lockfile could not be serialized to JSON to pass to
    /// the `afterAllResolved` pnpmfile hook.
    #[display("Failed to serialize lockfile for the afterAllResolved hook: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_AFTER_ALL_RESOLVED_SERIALIZE))]
    AfterAllResolvedSerialize(#[error(source)] serde_json::Error),

    /// The pnpmfile's `getCustomResolvers` hook threw while loading custom
    /// resolvers. A throwing custom-resolver hook aborts the install.
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The pnpmfile threw while loading its custom `fetchers` export.
    /// Same fatality rule as [`Self::CustomResolverHook`] and the
    /// frozen-lockfile path's custom-fetcher load.
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomFetcherHook(#[error(not(source))] pnpm_hooks::HookError),

    /// A custom resolver's `shouldRefreshResolution` hook threw while
    /// checking whether to force re-resolution. A throwing hook aborts
    /// the install.
    #[display("{_0}")]
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

impl<DependencyGroupList> InstallWithFreshLockfile<'_, DependencyGroupList> {
    /// Execute the subroutine.
    ///
    /// Under the isolated linker the [`HoistedDependencies`] result
    /// carries the publicly/privately-hoisted alias map; under
    /// `nodeLinker: hoisted` it is empty (the hoisted linker writes the
    /// on-disk tree directly and reports its placements through
    /// [`InstallWithFreshLockfileResult::hoisted_locations`] instead).
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let InstallWithFreshLockfile {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            importer_manifests,
            lockfile_specifier_manifests,
            dependency_groups,
            // No longer consulted: `CreateVirtualStore`'s warm/cold-batch
            // shape dedups by snapshot key inside the rayon pass. Kept on
            // the struct so `Install::run` can keep passing it.
            resolved_packages: _,
            logged_methods,
            requester,
            catalogs,
            lockfile_dir,
            workspace_packages,
            update_checksums,
            wanted_lockfile,
            merge_wanted_lockfile,
            node_version,
            meta_cache,
            node_linker,
            supported_architectures,
            lockfile_only,
            skip_runtimes,
            dry_run,
            can_prompt,
            persist_policy_excludes,
            is_full_install,
            update_seed_policy,
            preferred_versions_override,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            deps_requiring_build_sink,
            pnpmfile_hook_override,
            deploy_manifest_hook,
            real_importer_ids,
            selected_importer_ids,
            current_lockfile,
            prior_hoisted_dependencies,
            prune_orphans,
            save_lockfile,
            manifest_spec_bumps,
            resolution_verifiers,
            mut lockfile_verification_gate,
        } = self;

        // Shared once so the per-edge `ResolveOptions` clones below stay
        // refcount bumps — see `ResolveOptions::workspace_packages`.
        let workspace_packages = workspace_packages.map(Arc::new);

        // The pnpr override when supplied, else the config's npmrc headers;
        // shared by every registry-touching resolver below.
        let auth_headers = auth_override.unwrap_or_else(|| Arc::clone(&config.auth_headers));
        let package_version_guard =
            resolution_observer.as_ref().and_then(|observer| observer.package_version_guard());
        let minimum_release_age_exclude_override = resolution_observer
            .as_ref()
            .and_then(|observer| observer.minimum_release_age_exclude_override());
        let can_fast_update_overrides = resolution_observer.is_none();
        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        let link_options = crate::shim_link_options(config, node_linker);
        let filtered_isolated =
            is_partial_workspace_selection(real_importer_ids, selected_importer_ids) && !is_hoisted;
        let verify_filtered_repair = matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile)
            && is_partial_workspace_selection(real_importer_ids, selected_importer_ids);
        // Materialise the caller's iterator into a `Vec` so the same
        // group set can be replayed into both the resolver (consumes
        // the iterator) and `SymlinkDirectDependencies` (needs to walk
        // each importer's per-group dep list again). Mirrors the
        // `dependency_groups.into_iter().collect()` shape
        // `install_frozen_lockfile.rs` uses for the same reason.
        // `Vec<DependencyGroup>` is at most a few enum variants so the
        // clone cost is negligible.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
        let include_transitive_optional_dependencies =
            include_transitive_optional_dependencies(is_full_install, &dependency_groups);

        let store_dir: &'static _ = &config.store_dir;

        // Eagerly create `files/00..ff` under the v11 store root so per-
        // tarball CAFS writes never pay a `create_dir_all` syscall on the
        // hot path.
        // See [`init_store_dir_best_effort`] for the error-degradation
        // policy shared with `create_virtual_store.rs`. Skipped under
        // `frozenStore`: the store is read-only and complete, so no
        // directory creation is attempted under its root.
        if !config.frozen_store {
            init_store_dir_best_effort(store_dir).await;
        }

        let resolver_setup::Registries { by_scope: registries, named: merged_registries_by_prefix } =
            resolver_setup::resolve_registries(config)?;

        // `resolutionMode` / `minimumReleaseAge` derivations. `time_based`
        // and `pick_lowest_direct` steer the deps-resolver's per-depth
        // version pick; `full_metadata` forces the npm resolver to fetch
        // per-version `time` fields so the time-based cutoff and the
        // no-downgrade trust check have publication dates; `published_by`
        // (+exclude) is the maturity cutoff. Shared with `pacquet add`'s
        // explicit-spec pre-resolution via [`PickPolicy`] so both pick the
        // same version.
        let crate::resolution_policy::PickPolicy {
            time_based,
            pick_lowest_direct,
            full_metadata,
            needs_full_metadata_for,
            published_by,
            published_by_exclude,
        } = crate::resolution_policy::PickPolicy::from_config_with_extra_excludes(
            config,
            minimum_release_age_exclude_override.as_deref(),
        )
        .map_err(InstallWithFreshLockfileError::MinimumReleaseAgeExclude)?;

        let resolver_setup::StoreIndexHandles {
            index: store_index,
            writer: store_index_writer,
            writer_task,
        } = resolver_setup::open_store_index_handles(config, store_dir).await;
        let store_index_ref = store_index.as_ref();

        let verified_files_cache = SharedVerifiedFilesCache::default();

        // Records package-status progress emitted by resolve-time
        // prefetches. `CreateVirtualStore` still emits `resolved` later,
        // but skips duplicate `fetched` / `found_in_store` statuses for
        // keys already reported here.
        let progress_reported = SharedReportedProgressKeys::default();

        let resolver_setup::ResolverChain {
            resolver,
            npm_resolver,
            fetch_locker,
            picked_manifest_cache,
            custom_resolvers: custom_resolvers_raw,
            custom_fetcher_session,
            pnpmfile_hook,
        } = resolver_setup::build_resolver_chain::<Reporter>(resolver_setup::ResolverChainInputs {
            config,
            store_dir,
            http_client_arc: &http_client_arc,
            tarball_mem_cache: &tarball_mem_cache,
            auth_headers: &auth_headers,
            meta_cache: &meta_cache,
            lockfile_dir,
            requester,
            supported_architectures,
            registries: &registries,
            needs_full_metadata_for: Arc::clone(&needs_full_metadata_for),
            registries_by_prefix: &merged_registries_by_prefix,
            full_metadata,
            wanted_lockfile,
            store_index: store_index_ref,
            store_index_writer: &store_index_writer,
            verified_files_cache: &verified_files_cache,
            progress_reported: &progress_reported,
            prefetch_downloads: !lockfile_only && !filtered_isolated,
            pnpmfile_hook_override,
            resolution_observer,
        })
        .await?;

        // `trustPolicy='no-downgrade'` config, threaded into every
        // resolve so the npm resolver re-applies the downgrade gate to
        // freshly picked versions. `full_metadata` above is already
        // forced on under this policy, so the picker hands the resolver
        // the per-version `time` + trust evidence the check reads.
        let trust_policy = match config.trust_policy {
            TrustPolicy::Off => None,
            TrustPolicy::NoDowngrade => Some(TrustPolicy::NoDowngrade),
        };
        let trust_policy_exclude = config
            .trust_policy_exclude
            .as_deref()
            .filter(|patterns| !patterns.is_empty())
            .map(pnpm_config::version_policy::create_package_version_policy)
            .transpose()
            .map_err(InstallWithFreshLockfileError::TrustPolicyExclude)?;

        let manifest_transforms::ManifestTransforms {
            parsed_overrides,
            resolved_overrides,
            package_extensions_checksum,
            versions_overrider,
            manifest_hook,
            overrides_hook,
            override_bare_specifier,
            effective_importer_manifests,
        } = manifest_transforms::build_manifest_transforms(
            config,
            &catalogs,
            lockfile_dir,
            &importer_manifests,
            deploy_manifest_hook,
        )?;
        let importer_manifests: BTreeMap<String, &PackageManifest> =
            if effective_importer_manifests.is_empty() {
                importer_manifests
            } else {
                effective_importer_manifests
                    .iter()
                    .map(|(id, manifest)| (id.clone(), manifest))
                    .collect()
            };

        let fixed_wanted_lockfile = if matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile) {
            wanted_lockfile.cloned().map(|mut lockfile| {
                lockfile.prepare_for_fix();
                lockfile
            })
        } else {
            None
        };
        let wanted_lockfile = fixed_wanted_lockfile.as_ref().or(wanted_lockfile);

        let (preferred_versions_seed, preferred_versions_seeds_by_importer) =
            resolve::preferred_versions_seeds(
                &update_seed_policy,
                wanted_lockfile,
                &importer_manifests,
                preferred_versions_override.as_ref(),
            );
        // Resolve `pnpm-workspace.yaml`'s `patchedDependencies` once
        // per install. The resolver consults the grouped record at
        // every per-node lookup to attach `(patch_hash=<hash>)` to the
        // matched package's `pkgIdWithPatchHash`.
        let patched_dependencies = config
            .resolved_patched_dependencies()
            .map_err(InstallWithFreshLockfileError::ResolvePatchedDependencies)?
            .map(Arc::new);
        // The verbatim `patchedDependencies` key → patch-file-hash map
        // recorded in the lockfile's top-level `patchedDependencies`
        // block. Computed separately from the grouped record above
        // (which buckets by package name) so the user's exact keys
        // survive into the lockfile.
        let patched_dependency_hashes = config
            .patched_dependency_hashes()
            .map_err(InstallWithFreshLockfileError::CalcPatchHashes)?;

        // Loop per workspace project. Each importer gets its own
        // resolve_importer call with its own `project_dir` so
        // `workspace:` / `link:` resolutions compute paths relative
        // to the consuming project; the shared `meta_cache`,
        // `fetch_locker`, and `picked_manifest_cache` keep the
        // packument and version-pick work amortized across importers.
        // One shared resolution context, per-importer direct-deps
        // slices.
        // Kept past the resolver hand-off (which consumes `pnpmfile_hook`) so
        // the `afterAllResolved` hook can transform the lockfile before it is
        // written.
        let after_all_resolved_hook = pnpmfile_hook.clone();
        // Pre-bind the reporter, project prefix, and pnpmfile path into the
        // `context.log(...)` sinks so the resolver and lockfile writer stay
        // reporter-agnostic. Each hook's `context.log` is forwarded to the
        // `pnpm:hook` channel.
        let pnpmfile_path =
            pnpmfile_hook.as_ref().and_then(|hook| hook.source_path()).map(Path::to_path_buf);
        let read_package_log = pnpmfile_path
            .as_ref()
            .map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, "readPackage"));
        let after_all_resolved_log = pnpmfile_path
            .as_ref()
            .map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, "afterAllResolved"));

        if let Some(ref hook) = pnpmfile_hook {
            resolve::run_pre_resolution_hook::<Reporter>(
                hook,
                config,
                lockfile_dir,
                wanted_lockfile,
            )
            .await;
        }

        // `pacquet update` must re-resolve its targets to highest-in-range,
        // so suppress reuse for them (and their subtrees). Custom resolvers
        // may widen this to `None` via `shouldRefreshResolution`.
        let (mut update_reuse_scope, mut update_reuse_scopes_by_importer) =
            update_reuse_scopes(&update_seed_policy);

        // A throwing hook propagates and aborts.
        if let Some(lockfile) = wanted_lockfile
            && crate::check_custom_resolver_force_resolve::check_custom_resolver_force_resolve(
                &custom_resolvers_raw,
                lockfile,
            )
            .await
            .map_err(InstallWithFreshLockfileError::CustomResolverForceResolve)?
        {
            update_reuse_scope = pnpm_resolving_deps_resolver::UpdateReuseScope::None;
            update_reuse_scopes_by_importer.clear();
        }

        // Captured for the pnpm/pnpm#10433 guard in the fresh-lockfile
        // builder (`build_importer`): it needs the previous run's importer
        // entries and this run's final update scope, but `update_reuse_scope`
        // is moved into the resolver options below and `wanted_lockfile` is
        // later shadowed by the freshly built lockfile.
        // Withheld when `dedupe_injected_deps` is off, since the guard only
        // compensates for that pass not running on every re-resolution path.
        let guard_previous_importers: Option<&HashMap<String, pnpm_lockfile::ProjectSnapshot>> =
            merge_wanted_lockfile
                .filter(|_| config.dedupe_injected_deps)
                .map(|lockfile| &lockfile.importers);
        let guard_update_reuse_scope = update_reuse_scope.clone();
        let guard_update_reuse_scopes_by_importer = update_reuse_scopes_by_importer.clone();

        let shared_resolve_options = resolve::SharedResolveOptions {
            config,
            lockfile_dir,
            published_by,
            published_by_exclude: published_by_exclude.clone(),
            trust_policy,
            trust_policy_exclude: trust_policy_exclude.clone(),
            package_version_guard: package_version_guard.clone(),
            workspace_packages: workspace_packages.clone(),
            update_checksums,
            update_behavior: if matches!(update_seed_policy, UpdateSeedPolicy::RefreshRevisions) {
                pnpm_resolving_resolver_base::UpdateBehavior::Patches
            } else {
                pnpm_resolving_resolver_base::UpdateBehavior::Off
            },
        };
        let lockfile_reuse_seed = resolve::lockfile_reuse_seed(resolve::ReuseSeedInputs {
            config,
            catalogs: &catalogs,
            wanted_lockfile,
            package_extensions_checksum: package_extensions_checksum.as_deref(),
            parsed_overrides: parsed_overrides.as_deref(),
            resolved_overrides: resolved_overrides.as_ref(),
            manifest_hook: manifest_hook.clone(),
            overrides_hook: overrides_hook.clone(),
            fast_override_eligible: pnpmfile_hook.is_none()
                && custom_resolvers_raw.is_empty()
                && patched_dependencies.is_none()
                && can_fast_update_overrides,
            npm_resolver: &*npm_resolver,
            resolve_options: &shared_resolve_options
                .build(lockfile_dir.to_path_buf(), Arc::clone(&preferred_versions_seed)),
            registries: &registries,
        })
        .await;
        // Reused subtrees never stream their manifests through the
        // versions overrider, so only a resolution with no reuse at all
        // collects the complete declared-range set the convergence
        // staleness check needs.
        let full_resolution = full_resolution_required(
            lockfile_reuse_seed.is_some(),
            importer_manifests.keys().map(String::as_str),
            &update_reuse_scope,
            &update_reuse_scopes_by_importer,
        );
        let reuse_lockfile_subtrees = lockfile_reuse_seed.is_some();
        // A withheld seed means config drift the fast rewrites cannot
        // absorb, so recorded subtrees must re-resolve — but the prior
        // lockfile still pins the edges the drift does not reach (see
        // `WorkspaceResolveOptions::reuse_lockfile_subtrees`).
        let resolution_lockfile = lockfile_reuse_seed
            .or_else(|| wanted_lockfile.map(|lockfile| Arc::new(lockfile.clone())));

        let phase_start = std::time::Instant::now();
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: lockfile_dir.display().to_string(),
            stage: Stage::ResolutionStarted,
        }));
        let workspace_result = resolve::run_resolve_pass::<Reporter>(resolve::ResolvePassInputs {
            config,
            resolver: &*resolver,
            importer_manifests: &importer_manifests,
            dependency_groups: &dependency_groups,
            catalogs: &catalogs,
            lockfile_dir,
            shared_resolve_options: &shared_resolve_options,
            preferred_versions_seed: &preferred_versions_seed,
            preferred_versions_seeds_by_importer: &preferred_versions_seeds_by_importer,
            override_bare_specifier,
            patched_dependencies: patched_dependencies.clone(),
            manifest_hook,
            overrides_hook,
            pnpmfile_hook,
            read_package_log,
            pick_lowest_direct,
            time_based,
            published_by,
            resolution_lockfile,
            reuse_lockfile_subtrees,
            update_reuse_scope,
            update_reuse_scopes_by_importer,
            update_depth: update_seed_policy.max_depth(),
            registries,
            registries_by_prefix: merged_registries_by_prefix.clone(),
        })
        .await?;
        crate::minimum_release_age::handle_minimum_release_age_violations::<Reporter>(
            config,
            lockfile_dir,
            &workspace_result.merged_tree.policy_violations,
            can_prompt && !dry_run,
            persist_policy_excludes && !dry_run,
        )
        .await
        .map_err(InstallWithFreshLockfileError::MinimumReleaseAge)?;
        // Only in the fresh-lockfile path — frozen lockfile trusts recorded
        // patches. pnpm's importer-count gate admits an unfiltered run, or a
        // filtered run whose root-augmented selection and previous wanted
        // lockfile both cover the complete workspace. A first filtered
        // install has no complete previous lockfile and skips this check.
        let verify_patch_usage = match selected_importer_ids {
            None => true,
            Some(selected_importer_ids) => {
                !is_partial_workspace_selection(real_importer_ids, Some(selected_importer_ids))
                    && merge_wanted_lockfile.is_some_and(|wanted_lockfile| {
                        wanted_lockfile.importers.len() == selected_importer_ids.len()
                    })
            }
        };
        if let Some(ref deps) = patched_dependencies
            && verify_patch_usage
        {
            match pnpm_patching::verify_patches(
                deps,
                &workspace_result.merged_tree.applied_patches.iter().cloned().collect(),
                config.allow_unused_patches,
            ) {
                Ok(None) => {}
                Ok(Some(warning)) => {
                    Reporter::emit(&LogEvent::Global(GlobalLog {
                        level: LogLevel::Warn,
                        message: warning.to_string(),
                    }));
                }
                Err(err) => {
                    return Err(InstallWithFreshLockfileError::UnusedPatch(err));
                }
            }
        }
        let total_nodes = workspace_result.peers.graph.len();
        // Hand the per-importer issues to the programmatic caller
        // before the graph is consumed below.
        if let Some(sink) = &peer_issues_sink {
            *sink.lock().expect("peer-issues sink lock poisoned") =
                workspace_result.peers.peer_dependency_issues_by_importer.clone();
        }
        for (importer_id, issues) in &workspace_result.peers.peer_dependency_issues_by_importer {
            tracing::warn!(
                target: "pacquet::install",
                importer_id = %importer_id,
                missing = issues.missing.len(),
                bad = issues.bad.len(),
                "Peer dependency issues detected (issue renderer not ported yet)",
            );
        }
        let merged_graph = workspace_result.peers.graph;
        let direct_by_importer = workspace_result.peers.direct_dependencies_by_importer;
        let resolved_time = workspace_result.time;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "resolve_workspace",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            importers = importer_manifests.len(),
            nodes = total_nodes,
            "phase complete",
        );

        // Only a full resolution walks every manifest through the
        // versions overrider, making the collected declared ranges
        // complete enough for the staleness verdict; partial (reuse-
        // seeded) resolutions must stay silent to avoid false positives
        // from unseen ranges. Runs before the resolver chain is dropped
        // so the per-range picks reuse the still-warm packument cache.
        if full_resolution
            && let (Some(parsed), Some(overrider)) =
                (parsed_overrides.as_ref(), versions_overrider.as_ref())
        {
            resolve::warn_stale_convergence_overrides::<Reporter>(
                &*npm_resolver,
                parsed,
                overrider,
                lockfile_dir,
                published_by,
                published_by_exclude.as_ref(),
            )
            .await;
        }

        // Shrink the packument, fetch-locker, and picked-manifest caches
        // before the install pass pulls more tarballs into the CAFS.
        // Each binding needs its own drop: the deno- and bun-resolvers
        // were handed clones of the same `npm_resolver` `Arc`, so
        // dropping the chain alone leaves a strong reference behind.
        drop(resolver);
        drop(npm_resolver);
        drop(meta_cache);
        drop(fetch_locker);
        drop(picked_manifest_cache);

        // Must come after every `pnpm:deprecation` emit — the default
        // reporter flushes its transitive-deprecation buffer on this event.
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: lockfile_dir.display().to_string(),
            stage: Stage::ResolutionDone,
        }));

        // Compute the `pnpmfileChecksum` once for both lockfile-build
        // paths below: the hash of the project's `.pnpmfile.{cjs,mjs}`
        // when it exports hooks, `None` otherwise. Resolution has already spawned the
        // pnpmfile worker (every `readPackage` runs through it), so the
        // gate query is cheap here.
        let pnpmfile_checksum: Option<String> = match after_all_resolved_hook.as_ref() {
            Some(hook) => hook.calculate_pnpmfile_checksum().await,
            None => None,
        };

        // `--lockfile-only`: the graph is resolved, so build and write
        // `pnpm-lock.yaml` and return before any materialization. Nothing
        // was prefetched, and there is no `node_modules`,
        // `.modules.yaml`, or current lockfile — a lockfile-only resolve
        // pass.
        if lockfile_only {
            if let Some(gate) = lockfile_verification_gate.take() {
                gate.wait().await.map_err(InstallWithFreshLockfileError::LockfileVerification)?;
            }
            let built_lockfile = build_lockfile(FreshLockfileBuildOptions {
                config,
                importer_manifests: &importer_manifests,
                lockfile_specifier_manifests: lockfile_specifier_manifests.as_ref(),
                graph: &merged_graph,
                direct_by_importer: &direct_by_importer,
                resolved_overrides: resolved_overrides.clone(),
                catalogs: &catalogs,
                pnpmfile_checksum: pnpmfile_checksum.as_deref(),
                patched_dependency_hashes: patched_dependency_hashes.as_ref(),
                previous_importers: guard_previous_importers,
                update_reuse_scope: guard_update_reuse_scope.clone(),
                update_reuse_scopes_by_importer: guard_update_reuse_scopes_by_importer.clone(),
                wanted_lockfile,
                merge_wanted_lockfile,
                real_importer_ids,
                selected_importer_ids,
                lockfile_dir,
                resolved_time,
                manifest_spec_bumps,
                versions_overrider: versions_overrider.as_deref(),
            })?;
            if verify_filtered_repair {
                verify_merged_repair::<Reporter>(&built_lockfile, resolution_verifiers).await?;
            }
            return finish_lockfile_only::<Reporter>(LockfileOnlyOptions {
                built_lockfile,
                config,
                lockfile_dir,
                requester,
                dry_run,
                save_lockfile,
                after_all_resolved_hook: after_all_resolved_hook.as_ref(),
                after_all_resolved_log,
                store_index_writer,
                writer_task,
            })
            .await;
        }

        let allow_build_policy = AllowBuildPolicy::from_config(config)
            .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)?;
        // Built unconditionally: the layout and the bin-link pass both
        // read its `snapshots:` / `packages:` maps, the build costs
        // ~3 ms on the alotta-files fixture, and it is what gets saved
        // below anyway.
        let phase_start = std::time::Instant::now();
        let mut built_lockfile = build_lockfile(FreshLockfileBuildOptions {
            config,
            importer_manifests: &importer_manifests,
            lockfile_specifier_manifests: lockfile_specifier_manifests.as_ref(),
            graph: &merged_graph,
            direct_by_importer: &direct_by_importer,
            resolved_overrides: resolved_overrides.clone(),
            catalogs: &catalogs,
            pnpmfile_checksum: pnpmfile_checksum.as_deref(),
            patched_dependency_hashes: patched_dependency_hashes.as_ref(),
            previous_importers: guard_previous_importers,
            update_reuse_scope: guard_update_reuse_scope.clone(),
            update_reuse_scopes_by_importer: guard_update_reuse_scopes_by_importer.clone(),
            wanted_lockfile,
            merge_wanted_lockfile,
            real_importer_ids,
            selected_importer_ids,
            lockfile_dir,
            resolved_time,
            manifest_spec_bumps,
            versions_overrider: versions_overrider.as_deref(),
        })?;
        if verify_filtered_repair {
            if let Some(gate) = lockfile_verification_gate.take() {
                gate.wait().await.map_err(InstallWithFreshLockfileError::LockfileVerification)?;
            }
            verify_merged_repair::<Reporter>(&built_lockfile, resolution_verifiers).await?;
        }
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "build_fresh_lockfile",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );
        let included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };
        let initial_materialization_ids = selected_importer_ids.map(|selected_importer_ids| {
            if is_hoisted {
                built_lockfile.importers.keys().cloned().collect()
            } else {
                selected_importer_ids.clone()
            }
        });
        let empty_skipped = SkippedSnapshots::new();
        let initial_materialization = initial_materialization_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                &built_lockfile,
                lockfile_dir,
                importer_ids,
                included,
                &empty_skipped,
            )
        });
        let initial_materialization_lockfile =
            initial_materialization.as_ref().map_or(&built_lockfile, |closure| &closure.lockfile);
        let needs_installability_check = !config.force
            && initial_materialization_lockfile.packages.as_ref().is_some_and(|packages| {
                initial_materialization_lockfile.snapshots.as_ref().is_some_and(|snapshots| {
                    crate::any_installability_constraint(snapshots, packages)
                })
            });
        let installability_host =
            pnpm_deps_restorer::materialization_plan::detect_installability_host(
                needs_installability_check,
                config.engine_strict,
                node_version,
                supported_architectures,
            )
            .await;
        let host_node = installability_host
            .as_ref()
            .map(pnpm_deps_restorer::materialization_plan::HostNode::from);

        let (engine_name, deferred_engine_name) =
            pnpm_deps_restorer::materialization_plan::resolve_engine_name(
                config.enable_global_virtual_store,
                initial_materialization_lockfile.snapshots.as_ref(),
                host_node.as_ref(),
            )
            .await;
        let layout_engine_name =
            if config.enable_global_virtual_store { engine_name.as_deref() } else { None };
        let phase_start = std::time::Instant::now();
        let layout = VirtualStoreLayout::new(
            config,
            layout_engine_name,
            initial_materialization_lockfile.snapshots.as_ref(),
            initial_materialization_lockfile.packages.as_ref(),
            Some(&allow_build_policy),
            Some(lockfile_dir),
        );
        let dir_clone_cache = pnpm_deps_restorer::DirCloneCache::build(
            config,
            node_linker,
            match &deferred_engine_name {
                Some(deferred) => pnpm_deps_restorer::EngineNameSource::Pending(deferred.shared()),
                None => pnpm_deps_restorer::EngineNameSource::Ready(engine_name.clone()),
            },
            initial_materialization_lockfile.snapshots.as_ref(),
            initial_materialization_lockfile.packages.as_ref(),
            Some(&allow_build_policy),
            Some(lockfile_dir),
        );
        if config.enable_global_virtual_store {
            tracing::info!(
                target: "pacquet::install::phase",
                phase = "virtual_store_layout_new",
                elapsed_ms = phase_start.elapsed().as_millis() as u64,
                "phase complete",
            );
        }

        let closure_importer_ids: std::collections::HashSet<String> =
            built_lockfile.importers.keys().cloned().collect();
        let mut skipped = pnpm_deps_restorer::materialization_plan::compute_skip_set::<Reporter>(
            pnpm_deps_restorer::materialization_plan::SkipSetInputs {
                requester,
                importers: &initial_materialization_lockfile.importers,
                snapshots: initial_materialization_lockfile.snapshots.as_ref(),
                packages: initial_materialization_lockfile.packages.as_ref(),
                installability_host: installability_host.as_ref(),
                // The fresh path has just re-resolved the graph, so the
                // previous run's verdicts may no longer hold.
                seed: SkippedSnapshots::new(),
                // Only a full install's `dependency_groups` carries a
                // `--no-optional` intent: a partial run either passes
                // every direct group (`add`, `remove`, `update`) or
                // narrows them for its own reasons (`fetch --dev`,
                // `rebuild`) and must keep its transitive optionals.
                exclude_optional: !include_transitive_optional_dependencies,
                skip_runtimes,
                closure_lockfile: &built_lockfile,
                closure_root: lockfile_dir,
                closure_importer_ids: &closure_importer_ids,
                included,
            },
        )
        .map_err(InstallWithFreshLockfileError::Installability)?;

        let final_materialization = initial_materialization_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                &built_lockfile,
                lockfile_dir,
                importer_ids,
                included,
                &skipped,
            )
        });
        let materialization_lockfile =
            final_materialization.as_ref().map_or(&built_lockfile, |closure| &closure.lockfile);
        let materialization_importer_ids = final_materialization.as_ref().map_or_else(
            || built_lockfile.importers.keys().cloned().collect(),
            |closure| closure.importer_ids.clone(),
        );
        let project_anchor_importer_ids = match selected_importer_ids {
            Some(selected_importer_ids) if is_hoisted => selected_importer_ids.clone(),
            Some(_) => materialization_importer_ids.clone(),
            None => materialization_importer_ids.clone(),
        };

        // Materialise the virtual store via the same phased
        // warm/cold-batch pipeline the frozen-lockfile path uses. The
        // phased pipeline in `CreateVirtualStore` runs a single
        // `par_iter` over every warm snapshot at once, which closes the
        // ~94% wall-time gap to pnpm on the full-resolution-warm scenario
        // without regressing the cold-cache or frozen-lockfile paths.
        //
        let phase_start = std::time::Instant::now();
        let CreateVirtualStoreOutput {
            package_manifests,
            // Consumed by the build phase below to drive the
            // side-effects-cache `is_built` gate and the
            // `requiresBuild` decision per snapshot.
            side_effects_maps_by_snapshot,
            requires_build_by_snapshot,
            materialized_snapshots,
            // Optional snapshots whose fetch was swallowed. Folded into
            // the live skip set below so the symlink, bin-link, and build
            // phases observe them as absent — matching the frozen path
            // and avoiding dangling links / build attempts on a slot that
            // was never extracted.
            fetch_failed,
            // Populated only under `node_linker == Hoisted`; consumed by
            // the hoisted-linker pass below to materialize the on-disk
            // tree. `None` for the isolated linker.
            cas_paths_by_pkg_id,
            artifact_pin_records,
        } = CreateVirtualStore {
            http_client,
            config,
            packages: materialization_lockfile.packages.as_ref(),
            snapshots: materialization_lockfile.snapshots.as_ref(),
            current_snapshots: current_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()),
            current_packages: current_lockfile.and_then(|lockfile| lockfile.packages.as_ref()),
            layout: &layout,
            logged_methods,
            requester,
            store_index_writer: &store_index_writer,
            store_context: Some(pnpm_deps_restorer::CreateVirtualStoreStoreContext {
                index: store_index_ref,
                verified_files_cache: &verified_files_cache,
            }),
            cas_prefetch: None,
            allow_build_policy: &allow_build_policy,
            skipped: &skipped,
            include_optional_dependencies: include_transitive_optional_dependencies,
            supported_architectures,
            workspace_root: lockfile_dir,
            node_linker,
            dir_clone_cache: dir_clone_cache.as_ref(),
            progress_reported: &progress_reported,
            // Share the resolve-time prefetcher's in-flight downloads with
            // the cold batch. The `PrefetchingResolver` streams each
            // tarball into `tarball_mem_cache` keyed by URL; the cold
            // batch's only on-disk dedup is the store-index row, which the
            // prefetcher's writer commits asynchronously. Without the
            // shared cache a snapshot whose prefetch hasn't committed its
            // row yet is classified cold and re-downloaded — a race that
            // routing the cold batch through the mem cache fixes by
            // reusing the in-flight download instead.
            tarball_mem_cache: Some(&tarball_mem_cache),
            custom_fetcher_session: custom_fetcher_session.as_ref(),
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

        // The concurrent pre-resolve verification of the existing
        // lockfile must have its verdict before anything sensitive: the
        // symlink / bin-link phases, the dependency builds, and the
        // lockfile save below all run on a trusted lockfile only.
        if let Some(gate) = lockfile_verification_gate.take() {
            gate.wait().await.map_err(InstallWithFreshLockfileError::LockfileVerification)?;
        }

        // Fold fetch-failure swallows into the skip set before the
        // symlink / bin-link / build phases, mirroring the frozen path.
        for key in fetch_failed {
            skipped.add_fetch_failed(key);
        }

        // The store-index writer stays open past `CreateVirtualStore` so
        // the build phase can persist side-effects-cache rows; it is
        // dropped and drained after `run_build_phase`.

        // See `linking::run_link_phase` for why this anchors on
        // `modules_dir.parent()` rather than `lockfile_dir`.
        let symlink_root: &Path = config.modules_dir.parent().unwrap_or(lockfile_dir);

        let project_manifests_for_link: Vec<(std::path::PathBuf, &PackageManifest)> =
            importer_manifests
                .iter()
                .filter(|(id, _)| project_anchor_importer_ids.contains(id.as_str()))
                .map(|(id, manifest)| (lockfile_dir.join(id), *manifest))
                .collect();
        let package_map_project_manifests: Vec<(std::path::PathBuf, &PackageManifest)> =
            importer_manifests
                .iter()
                .map(|(id, manifest)| (lockfile_dir.join(id), *manifest))
                .collect();
        let root_component_importers: std::collections::HashSet<String> = importer_manifests
            .iter()
            .filter(|(id, _)| project_anchor_importer_ids.contains(id.as_str()))
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits()
                    == Some(pnpm_deps_restorer::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(id, _)| id.clone())
            .collect();

        let pnpm_deps_restorer::linking::LinkPhaseOutput {
            hoisted_dependencies,
            hoisted_locations,
            hoisted_pkg_roots_by_key,
            publicly_hoisted_for_post_build,
        } = pnpm_deps_restorer::linking::run_link_phase::<Reporter>(
            pnpm_deps_restorer::linking::LinkPhaseInputs {
                symlink_root,
                trusted_importer_ids: &project_anchor_importer_ids,
                root_component_importers: &root_component_importers,
                sidecar_lockfile: materialization_lockfile,
                config,
                layout: &layout,
                lockfile: materialization_lockfile,
                current_lockfile,
                snapshots: materialization_lockfile.snapshots.as_ref(),
                materialized_snapshots: Some(&materialized_snapshots),
                packages: materialization_lockfile.packages.as_ref(),
                importers: &materialization_lockfile.importers,
                project_manifests: &project_manifests_for_link,
                package_map_project_manifests: &package_map_project_manifests,
                dependency_groups: &dependency_groups,
                package_manifests: &package_manifests,
                cas_paths_by_pkg_id,
                link_options: &link_options,
                workspace_root: lockfile_dir,
                requester,
                node_linker,
                is_hoisted,
                prune_orphans,
                prior_hoisted_dependencies,
                host_node: host_node.as_ref(),
                supported_architectures,
                logged_methods,
            },
            &mut skipped,
        )
        .map_err(InstallWithFreshLockfileError::LinkPhase)?;

        // `importing_done` fires once extraction and symlink linking
        // are complete, before the build phase. Reporters use it to
        // close the import progress display so the subsequent
        // `pnpm:lifecycle` events render in their own section.
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        // Resolve the deferred `node --version` probe (non-GVS path);
        // it overlapped `CreateVirtualStore` above. Falls back to the
        // synchronous value when the probe wasn't deferred.
        let engine_name = match deferred_engine_name {
            Some(deferred) => deferred.handle.await.ok().flatten(),
            None => engine_name,
        };

        let build_extra_env = build_extra_env(config, node_linker, lockfile_dir);

        // `CreateVirtualStore` keeps skipped snapshots out of this map, so
        // it holds only what the install put on disk. See
        // [`crate::DepsRequiringBuildSink`].
        if let Some(sink) = &deps_requiring_build_sink {
            let deps_requiring_build = requires_build_by_snapshot
                .iter()
                .filter(|(_, requires_build)| **requires_build)
                .map(|(snapshot_key, _)| snapshot_key.to_string())
                .collect();
            *sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(deps_requiring_build);
        }

        // Run lifecycle scripts, report ignored builds, and re-link
        // top-level bins — the same build phase the frozen path runs, so
        // `pacquet add esbuild` reports the blocked `esbuild` build (and
        // builds approved packages) exactly like `pnpm`. `workspace_root`
        // is the real lockfile dir (sets each script's `INIT_CWD`); the
        // post-build bin link anchors on `symlink_root` to match where
        // this path placed `node_modules`.
        let crate::BuildModulesOutput { ignored_builds, deferred_builds } =
            crate::install_frozen_lockfile::run_build_phase::<Reporter>(
                &crate::install_frozen_lockfile::BuildPhaseInputs {
                    config,
                    workspace_root: lockfile_dir,
                    top_level_bin_root: symlink_root,
                    layout: &layout,
                    snapshots: materialization_lockfile.snapshots.as_ref(),
                    packages: materialization_lockfile.packages.as_ref(),
                    importers: &materialization_lockfile.importers,
                    dependency_groups: &dependency_groups,
                    // Reuse the record resolved earlier for the resolver so the
                    // patch files aren't hashed a second time.
                    patch_groups: patched_dependencies.as_deref(),
                    allow_build_policy: &allow_build_policy,
                    side_effects_maps_by_snapshot: &side_effects_maps_by_snapshot,
                    requires_build_by_snapshot: &requires_build_by_snapshot,
                    materialized_snapshots: &materialized_snapshots,
                    engine_name: engine_name.as_deref(),
                    extra_env: &build_extra_env,
                    store_index_writer: &store_index_writer,
                    skipped: &skipped,
                    hoisted_pkg_roots_by_key: hoisted_pkg_roots_by_key.as_ref(),
                    is_hoisted,
                    publicly_hoisted_for_post_build: &publicly_hoisted_for_post_build,
                    logged_methods,
                    // The fresh-resolve path never serves an explicit
                    // `pacquet rebuild`; rebuilds always take the frozen path.
                    rebuild: None,
                    link_options: &link_options,
                },
            )
            .map_err(InstallWithFreshLockfileError::BuildPhase)?;

        // Drop the orchestration's writer handle so the channel closes
        // once the build phase's side-effects-cache rows are queued and
        // the task starts winding down. It is returned as
        // `store_index_teardown` and awaited by the install driver
        // after the tail writes it overlaps.
        drop(store_index_writer);

        let injected_deps = crate::collect_injected_deps(
            &layout,
            lockfile_dir,
            materialization_lockfile.snapshots.as_ref(),
            materialization_lockfile.packages.as_ref(),
            &skipped,
            is_hoisted.then_some(&hoisted_locations),
        );

        if config.lockfile && save_lockfile {
            for record in artifact_pin_records {
                if let Some(snapshot) = built_lockfile
                    .snapshots
                    .as_mut()
                    .and_then(|snapshots| snapshots.get_mut(&record.snapshot_key))
                {
                    snapshot.record_artifact_pin(
                        record.input_key,
                        record.owner,
                        record.platform_fingerprint,
                        record.envelope_digest,
                    );
                }
            }
        }

        // Saved after the build phase succeeds so a partial install can't
        // leave a lockfile pointing at slots that never landed on disk.
        let (wanted_lockfile, can_record_lockfile_verification) = if !config.lockfile {
            (None, false)
        } else if save_lockfile {
            let target = lockfile_dir.join(config.wanted_lockfile_name());
            let can_record_lockfile_verification = save_wanted_lockfile(
                &built_lockfile,
                &target,
                after_all_resolved_hook.as_ref(),
                after_all_resolved_log.clone(),
            )
            .await?;
            (Some(built_lockfile), can_record_lockfile_verification)
        } else {
            // Nothing was persisted, so there is no `pnpm-lock.yaml`
            // whose verification a later install could key off.
            (Some(built_lockfile), false)
        };

        Ok(InstallWithFreshLockfileResult {
            hoisted_dependencies,
            hoisted_locations,
            injected_deps,
            wanted_lockfile,
            can_record_lockfile_verification,
            ignored_builds,
            deferred_builds,
            skipped,
            store_index_teardown: writer_task,
        })
    }
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

struct LockfileOnlyOptions<'a> {
    built_lockfile: Lockfile,
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
    let LockfileOnlyOptions {
        built_lockfile,
        config,
        lockfile_dir,
        requester,
        dry_run,
        save_lockfile,
        after_all_resolved_hook,
        after_all_resolved_log,
        store_index_writer,
        writer_task,
    } = opts;

    let (wanted_lockfile, can_record_lockfile_verification) = if dry_run || !save_lockfile {
        (Some(built_lockfile), false)
    } else if config.lockfile {
        let can_record_lockfile_verification = save_wanted_lockfile(
            &built_lockfile,
            &lockfile_dir.join(config.wanted_lockfile_name()),
            after_all_resolved_hook,
            after_all_resolved_log,
        )
        .await?;
        (Some(built_lockfile), can_record_lockfile_verification)
    } else {
        (None, false)
    };

    // Close the writer cleanly even though no rows were written,
    // mirroring the materializing path: drop closes the channel, the
    // caller awaits the returned task after its own tail writes.
    drop(store_index_writer);

    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: requester.to_string(),
        stage: Stage::ImportingDone,
    }));
    Ok(InstallWithFreshLockfileResult {
        hoisted_dependencies: HoistedDependencies::new(),
        hoisted_locations: BTreeMap::new(),
        injected_deps: BTreeMap::new(),
        wanted_lockfile,
        can_record_lockfile_verification,
        ignored_builds: Vec::new(),
        deferred_builds: Vec::new(),
        skipped: SkippedSnapshots::new(),
        store_index_teardown: writer_task,
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

/// Build the [`Lockfile`] for `<lockfile_dir>/pnpm-lock.yaml` from the
/// merged resolver graph + per-importer direct-deps maps, with
/// [`dependencies_graph_to_lockfile()`] doing the wire-shape lifting.
struct FreshLockfileBuildOptions<'a> {
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
    /// The previous run's lockfile, spliced back over the importers a
    /// filtered install didn't resolve. `None` on a first install.
    wanted_lockfile: Option<&'a Lockfile>,
    /// Intact prior lockfile used when splicing back unselected importers.
    merge_wanted_lockfile: Option<&'a Lockfile>,
    /// Every importer the workspace declares, and the subset this run
    /// resolved. Both `Some` and unequal means the install is filtered,
    /// so the unselected importers keep their previous entries.
    real_importer_ids: Option<&'a std::collections::HashSet<String>>,
    selected_importer_ids: Option<&'a std::collections::HashSet<String>>,
    lockfile_dir: &'a Path,
    /// Publish dates this run resolved for the direct dependencies,
    /// layered over the ones [`Self::wanted_lockfile`] recorded. Empty
    /// unless the install resolved `time-based`.
    resolved_time: BTreeMap<String, String>,
    /// See [`InstallWithFreshLockfile::manifest_spec_bumps`].
    manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    /// The override set the run resolved under. Consulted only alongside
    /// [`Self::manifest_spec_bumps`], to leave a declaration an override
    /// governs where the project wrote it.
    versions_overrider: Option<&'a crate::VersionsOverrider>,
}

/// Build the fresh lockfile, then — under a filtered install — splice it
/// back over the importers this run did not resolve.
fn build_lockfile(
    opts: FreshLockfileBuildOptions<'_>,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    let wanted_lockfile = opts.merge_wanted_lockfile;
    let real_importer_ids = opts.real_importer_ids;
    let selected_importer_ids = opts.selected_importer_ids;
    let lockfile_dir = opts.lockfile_dir;
    let manifest_spec_bumps = opts.manifest_spec_bumps;
    let versions_overrider = opts.versions_overrider;
    let importer_manifests = opts.importer_manifests;
    let freshly_resolved = build_fresh_lockfile(opts).map_err(|error| {
        InstallWithFreshLockfileError::DependenciesGraphToLockfile(Box::new(error))
    })?;
    let mut built = match (real_importer_ids, selected_importer_ids) {
        (Some(real_importer_ids), Some(selected_importer_ids)) => {
            crate::merge_filtered_wanted_lockfile(
                wanted_lockfile,
                freshly_resolved,
                real_importer_ids,
                selected_importer_ids,
                lockfile_dir,
            )
            .map_err(InstallWithFreshLockfileError::MergeFilteredWantedLockfile)?
        }
        _ => freshly_resolved,
    };
    if let Some(bumps) = manifest_spec_bumps {
        let overridden =
            versions_overrider.filter(|overrider| !overrider.is_empty()).map(|overrider| {
                crate::manifest_spec_bumps::OverriddenDeclarations { overrider, importer_manifests }
            });
        crate::manifest_spec_bumps::apply_manifest_spec_bumps(
            &mut built,
            bumps,
            overridden.as_ref(),
        );
    }
    Ok(built)
}

fn build_fresh_lockfile(
    opts: FreshLockfileBuildOptions<'_>,
) -> Result<Lockfile, DependenciesGraphToLockfileError> {
    let FreshLockfileBuildOptions {
        wanted_lockfile,
        merge_wanted_lockfile: _,
        real_importer_ids: _,
        selected_importer_ids: _,
        lockfile_dir: _,
        resolved_time,
        config,
        importer_manifests,
        lockfile_specifier_manifests,
        graph,
        direct_by_importer,
        resolved_overrides,
        catalogs,
        pnpmfile_checksum,
        patched_dependency_hashes,
        previous_importers,
        update_reuse_scope,
        update_reuse_scopes_by_importer,
        manifest_spec_bumps: _,
        versions_overrider: _,
    } = opts;
    let mut importers = BTreeMap::new();
    for (id, manifest) in importer_manifests {
        let direct = direct_by_importer.get(id).cloned().unwrap_or_default();
        let manifest = lockfile_specifier_manifests
            .and_then(|manifests| manifests.get(id))
            .unwrap_or(*manifest);
        importers.insert(
            id.clone(),
            ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct },
        );
    }
    // Same merge the resolver chain performs; the config was already
    // validated at resolver construction, so skip re-validation here.
    let registries_by_prefix: HashMap<String, String> =
        pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX
            .iter()
            .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
            .chain(
                config.registries_by_prefix.iter().map(|(name, url)| (name.clone(), url.clone())),
            )
            .collect();
    let mut lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph,
        registry_options_by_url: &config.registry_options_by_url,
        auto_install_peers: config.auto_install_peers,
        dedupe_peers: config.dedupe_peers,
        exclude_links_from_lockfile: config.exclude_links_from_lockfile,
        inject_workspace_packages: config.inject_workspace_packages,
        peers_suffix_max_length: (config.peers_suffix_max_length
            != pnpm_config::default_peers_suffix_max_length())
        .then_some(config.peers_suffix_max_length),
        overrides: resolved_overrides,
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        patched_dependencies: patched_dependency_hashes.cloned(),
        package_extensions_checksum: compute_package_extensions_checksum(config),
        pnpmfile_checksum: pnpmfile_checksum.map(str::to_string),
        catalogs,
        registry: &config.registry,
        registries_by_prefix: &registries_by_prefix,
        lockfile_include_tarball_url: config.lockfile_include_tarball_url,
        previous_importers,
        previous_packages: wanted_lockfile.and_then(|lockfile| lockfile.packages.as_ref()),
        update_reuse_scope,
        update_reuse_scopes_by_importer,
        time: merge_recorded_time(wanted_lockfile, resolved_time),
    })?;
    if let (Some(previous), Some(snapshots)) = (
        wanted_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()),
        lockfile.snapshots.as_mut(),
    ) {
        for (key, snapshot) in snapshots {
            snapshot.artifact_pins =
                previous.get(key).and_then(|previous| previous.artifact_pins.clone());
        }
    }
    Ok(lockfile)
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

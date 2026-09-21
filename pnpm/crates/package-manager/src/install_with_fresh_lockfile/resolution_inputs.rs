use indexmap::IndexMap;
use pnpm_config::{Config, NeedsFullMetadataFor};
use pnpm_lockfile::Lockfile;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_resolving_npm_resolver::InMemoryPackageMetaCache;
use pnpm_resolving_resolver_base::PreferredVersions;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndexWriter,
};
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

pub(crate) struct WorkspaceLifecycleHooks {
    /// Consumed by the resolver; the caller keeps its own clone for the
    /// `afterAllResolved` hook.
    pub pnpmfile: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub read_package_log: Option<pnpm_hooks::LogFn>,
    pub finalized_package: Option<pnpm_resolving_deps_resolver::FinalizedPackageFn>,
}

pub(crate) struct ImporterVersionSeeds<'a> {
    pub shared: &'a Arc<PreferredVersions>,
    pub by_importer: &'a BTreeMap<String, Arc<PreferredVersions>>,
    /// See [`crate::resolution_policy::PickPolicy`].
    pub pick_lowest: bool,
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) struct ResolverStoreContext<'a> {
    pub dir: &'static StoreDir,
    pub index: Option<&'a SharedReadonlyStoreIndex>,
    pub index_writer: &'a Arc<StoreIndexWriter>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
}

pub(crate) struct ResolverFetchContext<'a> {
    pub http_client: &'a Arc<ThrottledClient>,
    pub git_sources: &'a Arc<pnpm_git_fetcher::GitSourceCache>,
    pub tarballs: &'a Arc<MemCache>,
    pub auth_headers: &'a Arc<AuthHeaders>,
    pub progress_reported: &'a SharedReportedProgressKeys,
    /// Whether a resolved tarball is prefetched — `false` for a run
    /// whose install pass will never ask for those bytes.
    pub prefetch: bool,
}

pub(crate) struct ResolverRegistryContext<'a> {
    pub named: &'a HashMap<String, String>,
    pub by_prefix: &'a HashMap<String, String>,
    pub cache: &'a Arc<InMemoryPackageMetaCache>,
    /// See `NpmResolver::full_metadata` — forced on when `time-based`
    /// resolution or the `no-downgrade` trust policy needs the
    /// per-version `time` field.
    pub full_metadata: bool,
    /// See `NpmResolver::needs_full_metadata_for` — the same question asked
    /// of one registry.
    pub needs_full_metadata: NeedsFullMetadataFor,
}

pub(crate) struct ResolverChainHooks {
    /// In-process hooks supplied by an embedder; `None` falls back to
    /// the on-disk `.pnpmfile.cjs` lookup.
    pub pnpmfile: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

pub(crate) struct ResolverChainProject<'a> {
    pub root: &'a Path,
    pub requester: &'a str,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub lockfile: Option<&'a Lockfile>,
}

pub(crate) struct ReuseLockfileInputs<'a> {
    /// The previous run's lockfile, the only reuse candidate.
    pub wanted: Option<&'a Lockfile>,
    /// An `Arc` handle to the same document, when the loader holds one;
    /// the reuse-verbatim path shares it instead of deep-copying.
    pub shared: Option<&'a Arc<Lockfile>>,
    pub extensions_checksum: Option<&'a str>,
    /// The `pnpmfileChecksum` this install would record, against the one
    /// the candidate holds. A pnpmfile's `readPackage` rewrites the
    /// manifests the recorded subtrees were resolved from, so a drifted
    /// checksum means they describe manifests this install no longer sees
    /// (<https://github.com/pnpm/pnpm/issues/3735>).
    pub pnpmfile_checksum: Option<&'a str>,
    pub parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    pub resolved_overrides: Option<&'a IndexMap<String, String>>,
}

impl<'a> ImporterVersionSeeds<'a> {
    pub(super) fn for_importer(&self, id: &str) -> &'a Arc<PreferredVersions> {
        self.by_importer.get(id).unwrap_or(self.shared)
    }
}

pub(crate) struct FreshLockfilePrior<'a> {
    /// The previous run's lockfile importer entries, threaded into the
    /// pnpm/pnpm#10433 guard so an untouched workspace dependency keeps
    /// its prior `link:` entry. `None` on a first install.
    pub importers: Option<&'a HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    /// How this install reuses the prior resolution (from the `pacquet
    /// update` seed policy), also consumed by the pnpm/pnpm#10433 guard.
    pub scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    /// Per-importer update scopes (the `ByImporter` policy of a recursive
    /// update), so the guard honors `pacquet update <name> --recursive`
    /// targeting per importer rather than the workspace-wide default.
    pub scopes_by_importer: BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
    /// The previous run's lockfile: its `packages:` seed the reuse and its
    /// `time:` is layered under this run's. `None` on a first install.
    pub lockfile: Option<&'a Lockfile>,
}

pub(crate) struct FreshLockfileResolution<'a> {
    pub graph: &'a pnpm_resolving_deps_resolver::DependenciesGraph,
    pub direct_by_importer:
        &'a BTreeMap<String, BTreeMap<String, pnpm_resolving_deps_resolver::DepPath>>,
    pub overrides: Option<IndexMap<String, String>>,
    pub include_peer_dependencies: bool,
    /// Publish dates this run resolved for the direct dependencies,
    /// layered over the ones [`FreshLockfilePrior::lockfile`] recorded. Empty
    /// unless the install resolved `time-based`.
    pub time: BTreeMap<String, String>,
}

pub(crate) struct LockfilePersistenceOptions<'a> {
    pub config: &'a Config,
    pub dir: &'a Path,
    pub dry_run: bool,
    pub save: bool,
    pub hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub log: Option<pnpm_hooks::LogFn>,
}

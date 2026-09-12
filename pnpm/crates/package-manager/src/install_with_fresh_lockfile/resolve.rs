//! The resolve phase: what the workspace resolution is given, the walk
//! itself, and the diagnostics that read its result.
//!
//! The read-package transform chain the resolve also consumes lives in
//! [`super::manifest_transforms`]; the resolver chain it walks is built
//! by [`super::resolver_setup`].

pub(super) use reuse::{ReuseSeedInputs, lockfile_reuse_seed, preferred_versions_seeds};

mod reuse;

use super::InstallWithFreshLockfileError;
use crate::VersionsOverrider;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::LogLevel;
use pnpm_resolving_deps_resolver::{
    DependencyOverrider, ManifestHook, ResolveImporterError, ResolveImporterOptions,
};
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions, Resolver};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

/// Call the pnpmfile's `preResolution` hook before resolution starts.
pub(super) async fn run_pre_resolution_hook<Reporter: pnpm_reporter::Reporter>(
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    config: &Config,
    lockfile_dir: &Path,
    wanted_lockfile: Option<&Lockfile>,
) {
    let wanted_lockfile_json = wanted_lockfile.map_or_else(
        || serde_json::json!({}),
        |lf| serde_json::to_value(lf).unwrap_or_else(|_| serde_json::json!({})),
    );
    let current_lockfile =
        Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir).ok().flatten();
    let exists_current_lockfile = current_lockfile.is_some();
    let current_lockfile_json = current_lockfile.map_or_else(
        || serde_json::json!({}),
        |lf| serde_json::to_value(lf).unwrap_or_else(|_| serde_json::json!({})),
    );
    let ctx = pnpm_hooks::PreResolutionHookContext {
        wanted_lockfile: wanted_lockfile_json,
        current_lockfile: current_lockfile_json,
        exists_current_lockfile,
        exists_non_empty_wanted_lockfile: wanted_lockfile
            .as_ref()
            .is_some_and(|lf| !lf.snapshots.as_ref().is_none_or(HashMap::is_empty)),
        lockfile_dir: lockfile_dir.to_string_lossy().to_string(),
        store_dir: config.store_dir.display().to_string(),
        registries: serde_json::json!(config.resolved_registries()),
    };
    hook.pre_resolution(
        ctx,
        pnpm_hooks::PreResolutionHookLogger {
            info: super::pre_resolution_log_fn::<Reporter>(lockfile_dir, LogLevel::Info),
            warn: super::pre_resolution_log_fn::<Reporter>(lockfile_dir, LogLevel::Warn),
        },
    )
    .await;
}

/// The [`ResolveOptions`] fields that are the same for every importer and
/// for the fast-override pre-pass. Only the consuming project's directory
/// and its preferred-versions seed vary — see [`Self::build`].
pub(super) struct SharedResolveOptions<'a> {
    pub config: &'a Config,
    pub lockfile_dir: &'a Path,
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
    pub published_by_exclude: Option<pnpm_config::version_policy::PackageVersionPolicy>,
    pub trust_policy: Option<pnpm_config::TrustPolicy>,
    pub trust_policy_exclude: Option<pnpm_config::version_policy::PackageVersionPolicy>,
    pub package_version_guard: Option<Arc<dyn pnpm_resolving_resolver_base::PackageVersionGuard>>,
    pub workspace_packages: Option<Arc<pnpm_resolving_resolver_base::WorkspacePackages>>,
    /// See [`super::FreshInputs::update_checksums`].
    pub update_checksums: bool,
    pub update_behavior: pnpm_resolving_resolver_base::UpdateBehavior,
}

impl SharedResolveOptions<'_> {
    pub(super) fn build(
        &self,
        project_dir: std::path::PathBuf,
        preferred_versions: Arc<PreferredVersions>,
    ) -> ResolveOptions {
        ResolveOptions {
            preferred_versions,
            default_tag: Some("latest".to_string()),
            published_by: self.published_by,
            published_by_exclude: self.published_by_exclude.clone(),
            trust_policy: self.trust_policy,
            trust_policy_exclude: self.trust_policy_exclude.clone(),
            trust_policy_ignore_after: self.config.trust_policy_ignore_after,
            package_version_guard: self.package_version_guard.clone(),
            project_dir,
            lockfile_dir: self.lockfile_dir.to_path_buf(),
            workspace_packages: self.workspace_packages.clone(),
            block_exotic_subdeps: self.config.block_exotic_subdeps,
            link_workspace_packages: self.config.link_workspace_packages,
            inject_workspace_packages: self.config.inject_workspace_packages,
            prefer_workspace_packages: self.config.prefer_workspace_packages,
            update_checksums: self.update_checksums,
            update: self.update_behavior,
            ..ResolveOptions::default()
        }
    }
}

/// Report the `pnpm.overrides` convergence entries whose pinned value is
/// now older than what every declared range would admit.
///
/// Only a full resolution walks every manifest through the versions
/// overrider, making the collected declared ranges complete enough for
/// the staleness verdict; a partial (reuse-seeded) resolution stays
/// silent rather than warn from unseen ranges. Call before the resolver
/// chain is dropped so the per-range picks reuse the still-warm packument
/// cache.
pub(super) async fn warn_stale_convergence_overrides<Reporter: pnpm_reporter::Reporter>(
    npm_resolver: &dyn pnpm_resolving_resolver_base::Resolver,
    parsed_overrides: &[pnpm_config_parse_overrides::VersionOverride],
    versions_overrider: &VersionsOverrider,
    lockfile_dir: &Path,
    published_by: Option<chrono::DateTime<chrono::Utc>>,
    published_by_exclude: Option<&pnpm_config::version_policy::PackageVersionPolicy>,
) {
    use crate::warn_on_stale_convergence_overrides as stale;

    let declared_ranges = versions_overrider.converge_declared_ranges();
    let resolve_options = ResolveOptions {
        project_dir: lockfile_dir.to_path_buf(),
        lockfile_dir: lockfile_dir.to_path_buf(),
        default_tag: Some("latest".to_string()),
        published_by,
        published_by_exclude: published_by_exclude.cloned(),
        ..ResolveOptions::default()
    };
    let stale_overrides = stale::find_stale_convergence_overrides(
        parsed_overrides,
        &declared_ranges,
        |name, range| {
            stale::resolve_best_admitted_version(npm_resolver, &resolve_options, name, range)
        },
    )
    .await;
    stale::warn_stale_convergence_overrides::<Reporter>(&stale_overrides);
}

pub(super) struct ResolvePassInputs<'a> {
    pub resolver: &'a dyn Resolver,
    pub importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    pub dependency_groups: &'a [DependencyGroup],
    pub walk: WorkspaceWalk,
    pub per_importer: ImporterInputs<'a>,
}

/// What the workspace walk consumes as a whole: the hooks and the reuse
/// policy the resolver takes ownership of.
pub(super) struct WorkspaceWalk {
    /// See
    /// [`WorkspaceResolveOptions::share_workspace_resolutions`](pnpm_resolving_deps_resolver::WorkspaceResolveOptions::share_workspace_resolutions).
    pub share_workspace_resolutions: bool,
    /// Consumed by the resolver; the caller keeps its own clone for the
    /// `afterAllResolved` hook.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub read_package_log: Option<pnpm_hooks::LogFn>,
    pub finalized_package: Option<pnpm_resolving_deps_resolver::FinalizedPackageFn>,
    pub time_based: bool,
    /// The prior lockfile the walk resolves against — the granted
    /// [`lockfile_reuse_seed`], or the raw wanted lockfile when the seed
    /// was withheld and only per-edge version pinning remains safe.
    pub resolution_lockfile: Option<Arc<Lockfile>>,
    /// Whether [`Self::resolution_lockfile`] is a granted reuse seed the
    /// walk may reuse whole subtrees from. `false` restricts it to
    /// per-edge version pinning.
    pub reuse_lockfile_subtrees: bool,
    pub update_reuse_scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    pub update_reuse_scopes_by_importer:
        BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
    pub update_depth: pnpm_resolving_deps_resolver::UpdateDepth,
    pub registries: HashMap<String, String>,
    pub registries_by_prefix: HashMap<String, String>,
}

/// What every importer's resolve reads, and the walk reads alongside.
pub(super) struct ImporterInputs<'a> {
    pub config: &'a Config,
    pub catalogs: &'a Catalogs,
    pub lockfile_dir: &'a Path,
    /// The `ResolveOptions` half every importer shares; the per-importer
    /// half is its own `project_dir` and preferred-versions seed.
    pub shared_resolve_options: &'a SharedResolveOptions<'a>,
    pub preferred_versions_seed: &'a Arc<PreferredVersions>,
    pub preferred_versions_seeds_by_importer: &'a BTreeMap<String, Arc<PreferredVersions>>,
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    pub patched_dependencies: Option<Arc<pnpm_patching::PatchGroupRecord>>,
    pub manifest_hook: Option<ManifestHook>,
    pub overrides_hook: Option<ManifestHook>,
    /// See [`crate::resolution_policy::PickPolicy`].
    pub pick_lowest_direct: bool,
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
}

impl ImporterInputs<'_> {
    fn peers_suffix_max_length(&self) -> usize {
        usize::try_from(self.config.peers_suffix_max_length).unwrap_or(usize::MAX)
    }

    fn resolve_importer_options(
        &self,
        importer: &pnpm_resolving_deps_resolver::WorkspaceImporter<'_>,
        modules_basename: &std::ffi::OsStr,
    ) -> ResolveImporterOptions {
        let preferred_versions = self
            .preferred_versions_seeds_by_importer
            .get(&importer.id)
            .unwrap_or(self.preferred_versions_seed);
        let project_dir = importer
            .manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
            .to_path_buf();
        ResolveImporterOptions {
            auto_install_peers: self.config.auto_install_peers,
            auto_install_peers_from_highest_match: self
                .config
                .auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root: self.config.resolve_peers_from_workspace_root,
            dedupe_peers: self.config.dedupe_peers,
            dedupe_peer_dependents: self.config.dedupe_peer_dependents,
            all_preferred_versions: Arc::clone(preferred_versions),
            override_bare_specifier: self.override_bare_specifier.clone(),
            patched_dependencies: self.patched_dependencies.clone(),
            // `resolve_workspace` computes the workspace-wide
            // time-based cutoff and overrides both of these per
            // importer; the values here only satisfy the struct.
            pick_lowest_direct: self.pick_lowest_direct,
            subdep_published_by: self.published_by,
            modules_dir: Some(project_dir.join(modules_basename)),
            base_opts: self
                .shared_resolve_options
                .build(project_dir, Arc::clone(preferred_versions)),
            catalogs: self.catalogs.clone(),
            exclude_links_from_lockfile: self.config.exclude_links_from_lockfile,
            lockfile_dir: Some(self.lockfile_dir.to_path_buf()),
            peers_suffix_max_length: self.peers_suffix_max_length(),
            catalog_server: false,
            manifest_hook: self.manifest_hook.clone(),
            overrides_hook: self.overrides_hook.clone(),
            pnpmfile_hook: None,
        }
    }
}

impl WorkspaceWalk {
    fn into_options<Reporter: pnpm_reporter::Reporter>(
        self,
        shared: &ImporterInputs<'_>,
    ) -> pnpm_resolving_deps_resolver::WorkspaceResolveOptions {
        let config = shared.config;
        pnpm_resolving_deps_resolver::WorkspaceResolveOptions {
            registry_context: pnpm_lockfile::RegistryContext {
                registries: self.registries,
                registries_by_prefix: self.registries_by_prefix,
                registry_options_by_url: config.registry_options_by_url.clone(),
            },
            dedupe_peers: config.dedupe_peers,
            dedupe_injected_deps: config.dedupe_injected_deps,
            dedupe_peer_dependents: config.dedupe_peer_dependents,
            resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
            exclude_links_from_lockfile: config.exclude_links_from_lockfile,
            lockfile_dir: shared.lockfile_dir.to_path_buf(),
            peers_suffix_max_length: shared.peers_suffix_max_length(),
            share_workspace_resolutions: self.share_workspace_resolutions,
            manifest_hook: shared.manifest_hook.clone(),
            overrides_hook: shared.overrides_hook.clone(),
            pnpmfile_hook: self.pnpmfile_hook,
            read_package_log: self.read_package_log,
            skipped_optional_log: Some(super::skipped_optional_log_fn::<Reporter>()),
            finalized_package: self.finalized_package,
            pick_lowest_direct: shared.pick_lowest_direct,
            time_based: self.time_based,
            wanted_lockfile: self.resolution_lockfile,
            reuse_lockfile_subtrees: self.reuse_lockfile_subtrees,
            update_reuse_scope: self.update_reuse_scope,
            update_reuse_scopes_by_importer: self.update_reuse_scopes_by_importer,
            update_depth: self.update_depth,
            auto_install_peers: config.auto_install_peers,
            allowed_deprecated_versions: config.allowed_deprecated_versions.clone(),
            deprecation_log: Some(super::deprecation_log_fn::<Reporter>()),
        }
    }
}

/// Walk every importer's dependencies through the resolver chain.
///
/// Each importer resolves with its own `project_dir` so `workspace:` /
/// `link:` resolutions compute paths relative to the consuming project,
/// while the resolver chain's shared packument, fetch-locker, and
/// picked-manifest caches keep the metadata and version-pick work
/// amortized across importers. `resolve_workspace` then runs the
/// cross-importer peer pass and applies `dedupeInjectedDeps`.
pub(super) async fn run_resolve_pass<Reporter: pnpm_reporter::Reporter>(
    inputs: ResolvePassInputs<'_>,
) -> Result<pnpm_resolving_deps_resolver::ResolveWorkspaceResult, InstallWithFreshLockfileError> {
    let ResolvePassInputs { resolver, importer_manifests, dependency_groups, walk, per_importer } =
        inputs;
    let workspace_importers: Vec<pnpm_resolving_deps_resolver::WorkspaceImporter<'_>> =
        importer_manifests
            .iter()
            .map(|(id, manifest)| pnpm_resolving_deps_resolver::WorkspaceImporter {
                id: id.clone(),
                manifest,
            })
            .collect();
    let modules_basename = per_importer
        .config
        .modules_dir
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("node_modules"), std::ffi::OsStr::to_os_string);
    pnpm_resolving_deps_resolver::resolve_workspace(
        resolver,
        &workspace_importers,
        dependency_groups,
        walk.into_options::<Reporter>(&per_importer),
        |importer| per_importer.resolve_importer_options(importer, &modules_basename),
    )
    .await
    .map_err(|err| match err {
        ResolveImporterError::Resolve(err) => {
            InstallWithFreshLockfileError::ResolveDependencyTree(err)
        }
        ResolveImporterError::RootDepManifest(err) => {
            InstallWithFreshLockfileError::RootDepManifest(err)
        }
    })
}

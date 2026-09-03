//! The resolve phase: what the workspace resolution is given, the walk
//! itself, and the diagnostics that read its result.
//!
//! The read-package transform chain the resolve also consumes lives in
//! [`super::manifest_transforms`]; the resolver chain it walks is built
//! by [`super::resolver_setup`].

use super::{ImporterUpdateSeedPolicy, InstallWithFreshLockfileError, UpdateSeedPolicy};
use crate::VersionsOverrider;
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::LogLevel;
use pnpm_resolving_deps_resolver::{
    DependencyOverrider, ManifestHook, ResolveImporterError, ResolveImporterOptions, UpdateTargets,
};
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions, Resolver};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

/// Seed `allPreferredVersions` from every importer's manifest + the
/// wanted lockfile's snapshots (when an existing one is present and is
/// being rewritten): the manifests contribute direct-dep specifiers, the
/// lockfile contributes concrete `(name, version)` pins that bump the
/// weight of an already-matching direct-dep entry. Without the
/// lockfile-side seed, every install on a stale lockfile would resolve
/// unrelated entries from scratch and lose their recorded pins; see
/// <https://pnpm.io/settings#preferfrozenlockfile>.
///
/// `pacquet update` withholds the pins for the names it is bumping so
/// they re-resolve to highest-in-range; everything else keeps its pin.
/// Manifest preferences remain workspace-wide. Returns the workspace-wide
/// seed plus the per-importer overrides
/// [`UpdateSeedPolicy::ByImporter`] asks for (empty otherwise).
///
/// The picker biases toward the seed so pins that still satisfy their
/// range survive the re-resolve. Each seed is behind an [`Arc`] so a
/// per-importer `ResolveOptions` shares it with a refcount bump rather
/// than deep-cloning the map.
pub(super) fn preferred_versions_seeds(
    update_seed_policy: &UpdateSeedPolicy,
    wanted_lockfile: Option<&Lockfile>,
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    overrides: Option<&PreferredVersions>,
) -> (Arc<PreferredVersions>, BTreeMap<String, Arc<PreferredVersions>>) {
    use pnpm_lockfile_preferred_versions::{
        get_preferred_versions_from_lockfile_and_manifests as from_lockfile,
        get_preferred_versions_from_lockfile_and_manifests_excluding as from_lockfile_excluding,
    };

    let manifests: Vec<&PackageManifest> = importer_manifests.values().copied().collect();
    let snapshots = wanted_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref());

    let mut workspace_seed = match update_seed_policy {
        UpdateSeedPolicy::KeepAll
        | UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::FixLockfile
        | UpdateSeedPolicy::RefreshRevisions
        | UpdateSeedPolicy::ByImporter { .. } => from_lockfile(snapshots, manifests.as_slice()),
        UpdateSeedPolicy::DropAll { .. } => from_lockfile(None, manifests.as_slice()),
        UpdateSeedPolicy::DropOnly { targets, .. } => {
            from_lockfile_excluding(snapshots, manifests.as_slice(), &withheld_pin(targets))
        }
    };

    // A per-importer policy carries its own overrides below. Layering them onto the
    // workspace seed as well would reach the importers that policy left out, moving
    // dependencies in projects the command never named.
    if !matches!(update_seed_policy, UpdateSeedPolicy::ByImporter { .. }) {
        merge_preferred_versions(&mut workspace_seed, overrides);
    }

    let mut by_importer = BTreeMap::new();
    if let UpdateSeedPolicy::ByImporter { policies, .. } = update_seed_policy {
        let mut drop_all_seed = None;
        let mut drop_only_seeds = HashMap::new();
        for (importer_id, policy) in policies {
            let seed = match policy {
                ImporterUpdateSeedPolicy::DropAll => {
                    Arc::clone(drop_all_seed.get_or_insert_with(|| {
                        let mut seed = from_lockfile(None, manifests.as_slice());
                        merge_preferred_versions(&mut seed, overrides);
                        Arc::new(seed)
                    }))
                }
                ImporterUpdateSeedPolicy::DropOnly(targets) => {
                    if let Some(seed) = drop_only_seeds.get(targets) {
                        Arc::clone(seed)
                    } else {
                        let seed = Arc::new({
                            let mut seed = from_lockfile_excluding(
                                snapshots,
                                manifests.as_slice(),
                                &withheld_pin(targets),
                            );
                            merge_preferred_versions(&mut seed, overrides);
                            seed
                        });
                        drop_only_seeds.insert(targets.clone(), Arc::clone(&seed));
                        seed
                    }
                }
            };
            by_importer.insert(importer_id.clone(), seed);
        }
    }

    (Arc::new(workspace_seed), by_importer)
}

/// Layer caller-supplied preferences onto a seed, per package name. A
/// selector present in both wins from `overrides`, which is how a version
/// named on the command line outranks the pin the lockfile seeded for it.
fn merge_preferred_versions(seed: &mut PreferredVersions, overrides: Option<&PreferredVersions>) {
    let Some(overrides) = overrides else { return };
    for (name, selectors) in overrides {
        seed.entry(name.clone()).or_default().extend(selectors.clone());
    }
}

/// Which lockfile pins `pacquet update` withholds from the seed, so its
/// targets re-resolve instead of settling back on their recorded version.
/// A target scoped to a version line withholds only that line's pins: the
/// other lines are not part of the update and must keep resolving to what
/// the lockfile recorded.
fn withheld_pin(targets: &UpdateTargets) -> impl Fn(&pnpm_lockfile::PackageKey) -> bool + '_ {
    |key| targets.covers(key.name.to_string().as_str(), key.suffix.version_semver())
}

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
    /// See [`super::InstallWithFreshLockfile::update_checksums`].
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
            always_try_workspace_packages: self.config.link_workspace_packages
                != pnpm_config::LinkWorkspacePackages::Off,
            inject_workspace_packages: self.config.inject_workspace_packages,
            prefer_workspace_packages: self.config.prefer_workspace_packages,
            update_checksums: self.update_checksums,
            update: self.update_behavior,
            ..ResolveOptions::default()
        }
    }
}

pub(super) struct ReuseSeedInputs<'a> {
    pub config: &'a Config,
    pub catalogs: &'a Catalogs,
    /// The previous run's lockfile, the only reuse candidate.
    pub wanted_lockfile: Option<&'a Lockfile>,
    /// An `Arc` handle to the same document, when the loader holds one;
    /// the reuse-verbatim path shares it instead of deep-copying.
    pub wanted_lockfile_shared: Option<&'a Arc<Lockfile>>,
    pub package_extensions_checksum: Option<&'a str>,
    pub parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    pub resolved_overrides: Option<&'a IndexMap<String, String>>,
    /// The extensions and overrides halves of the read-package chain.
    /// This path has no pnpmfile hook (see [`Self::fast_override_eligible`]),
    /// so they compose back into one hook.
    pub manifest_hook: Option<ManifestHook>,
    pub overrides_hook: Option<ManifestHook>,
    /// Whether the cheap override-rewrite pre-pass may run at all: it
    /// rewrites resolutions without consulting a hook, a custom
    /// resolver, or a patch, so any of those present rules it out. The
    /// pnpr server also opts out, since its per-resolution observer must
    /// see every edge.
    pub fast_override_eligible: bool,
    pub npm_resolver: &'a dyn pnpm_resolving_resolver_base::Resolver,
    pub resolve_options: &'a ResolveOptions,
    pub registries: &'a HashMap<String, String>,
}

/// Pick the prior lockfile the resolver may reuse already-resolved
/// subtrees from instead of re-resolving them against the registry (see
/// `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`).
///
/// A changed `catalogs` or `pnpm.overrides` block normally withholds the
/// seed entirely. Two shapes are cheap enough to rewrite in place
/// instead — a catalog edit and an exact generic registry override — and
/// each yields a dependency-shape-verified seed. They compose: the
/// catalog rewrite settles first and the override rewrite replays onto
/// its result, the order a resolution applies the two in. Every other
/// shape falls back to withholding.
pub(super) async fn lockfile_reuse_seed(inputs: ReuseSeedInputs<'_>) -> Option<Arc<Lockfile>> {
    use crate::{
        fast_update_catalog_versions::try_fast_update_catalog_versions,
        fast_update_catalogs::{FastCatalogUpdate, try_fast_update_catalogs},
        fast_update_overrides::{FastOverrideOptions, RewriteContext, try_fast_update_overrides},
    };

    let ReuseSeedInputs {
        config,
        catalogs,
        wanted_lockfile,
        wanted_lockfile_shared,
        package_extensions_checksum,
        parsed_overrides,
        resolved_overrides,
        manifest_hook,
        overrides_hook,
        fast_override_eligible,
        npm_resolver,
        resolve_options,
        registries,
    } = inputs;

    let overrides_use_catalogs = config
        .overrides
        .as_ref()
        .is_some_and(|overrides| overrides.values().any(|value| value.starts_with("catalog:")));
    let (catalogs_match, fast_catalog_seed) = match wanted_lockfile
        .map_or(FastCatalogUpdate::Unchanged, |lockfile| {
            try_fast_update_catalogs(lockfile, catalogs, overrides_use_catalogs)
        }) {
        FastCatalogUpdate::Unchanged => (true, None),
        FastCatalogUpdate::Updated(lockfile) => (false, Some(*lockfile)),
        FastCatalogUpdate::Unsupported => (false, None),
    };

    let lockfile = wanted_lockfile.filter(|lockfile| {
        lockfile.package_extensions_checksum.as_deref() == package_extensions_checksum
            && super::ignored_optional_dependencies_match(
                lockfile.ignored_optional_dependencies.as_deref(),
                config.ignored_optional_dependencies.as_deref(),
            )
    })?;
    let override_settings_match =
        super::overrides_match(lockfile.overrides.as_ref(), resolved_overrides);

    let rewrite_manifest_hook = super::compose_manifest_hooks(manifest_hook, overrides_hook);
    // A catalog move can change the effective value of an override whose
    // configured value is a `catalog:` reference — an effect no catalog
    // rewrite can express — so catalog drift under such an override goes to
    // the resolver. The override rewrite itself is safe under one: it runs
    // only once the catalogs are settled, and override values are compared
    // catalog-resolved, so a settled `catalog:` override shows no drift and
    // only the genuinely changed entries are rewritten.
    let can_rewrite_catalogs = fast_override_eligible && !overrides_use_catalogs;

    let catalog_rewrite = if catalogs_match {
        None
    } else if let Some(seed) = fast_catalog_seed {
        Some(seed)
    } else if can_rewrite_catalogs {
        // A catalog entry that now names a version the locked one cannot
        // satisfy left `catalogs_match` false with no seed above. Replacing
        // the package is the same rewrite an exact override performs.
        Some(
            try_fast_update_catalog_versions(
                &RewriteContext {
                    lockfile,
                    resolver: npm_resolver,
                    resolve_options,
                    manifest_hook: rewrite_manifest_hook.as_ref(),
                    registries,
                    registry_options_by_url: &config.registry_options_by_url,
                    lockfile_include_tarball_url: config.lockfile_include_tarball_url,
                },
                catalogs,
            )
            .await?,
        )
    } else {
        return None;
    };

    if override_settings_match {
        return Some(match catalog_rewrite {
            Some(rewritten) => Arc::new(rewritten),
            // `lockfile` is `wanted_lockfile` narrowed by the filter
            // above, so the loader's handle to it reuses the parsed
            // document verbatim.
            None => wanted_lockfile_shared.map_or_else(|| Arc::new(lockfile.clone()), Arc::clone),
        });
    }
    if !fast_override_eligible {
        return None;
    }
    let seed = try_fast_update_overrides(FastOverrideOptions {
        context: RewriteContext {
            lockfile: catalog_rewrite.as_ref().unwrap_or(lockfile),
            resolver: npm_resolver,
            resolve_options,
            manifest_hook: rewrite_manifest_hook.as_ref(),
            registries,
            registry_options_by_url: &config.registry_options_by_url,
            lockfile_include_tarball_url: config.lockfile_include_tarball_url,
        },
        parsed_overrides: parsed_overrides?,
        resolved_overrides: resolved_overrides?,
    })
    .await?;
    Some(Arc::new(seed))
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
    pub config: &'a Config,
    pub resolver: &'a dyn Resolver,
    /// See
    /// [`WorkspaceResolveOptions::share_workspace_resolutions`](pnpm_resolving_deps_resolver::WorkspaceResolveOptions::share_workspace_resolutions).
    pub share_workspace_resolutions: bool,
    pub importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    pub dependency_groups: &'a [DependencyGroup],
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
    /// Consumed by the resolver; the caller keeps its own clone for the
    /// `afterAllResolved` hook.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub read_package_log: Option<pnpm_hooks::LogFn>,
    pub finalized_package: Option<pnpm_resolving_deps_resolver::FinalizedPackageFn>,
    /// See [`crate::resolution_policy::PickPolicy`].
    pub pick_lowest_direct: bool,
    pub time_based: bool,
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
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
    let ResolvePassInputs {
        config,
        resolver,
        share_workspace_resolutions,
        importer_manifests,
        dependency_groups,
        catalogs,
        lockfile_dir,
        shared_resolve_options,
        preferred_versions_seed,
        preferred_versions_seeds_by_importer,
        override_bare_specifier,
        patched_dependencies,
        manifest_hook,
        overrides_hook,
        pnpmfile_hook,
        read_package_log,
        finalized_package,
        pick_lowest_direct,
        time_based,
        published_by,
        resolution_lockfile,
        reuse_lockfile_subtrees,
        update_reuse_scope,
        update_reuse_scopes_by_importer,
        update_depth,
        registries,
        registries_by_prefix,
    } = inputs;

    let workspace_importers: Vec<pnpm_resolving_deps_resolver::WorkspaceImporter<'_>> =
        importer_manifests
            .iter()
            .map(|(id, manifest)| pnpm_resolving_deps_resolver::WorkspaceImporter {
                id: id.clone(),
                manifest,
            })
            .collect();
    let peers_suffix_max_length =
        usize::try_from(config.peers_suffix_max_length).unwrap_or(usize::MAX);
    let modules_basename = config
        .modules_dir
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("node_modules"), std::ffi::OsStr::to_os_string);

    let workspace_opts = pnpm_resolving_deps_resolver::WorkspaceResolveOptions {
        registry_context: pnpm_lockfile::RegistryContext {
            registries,
            registries_by_prefix,
            registry_options_by_url: config.registry_options_by_url.clone(),
        },
        dedupe_peers: config.dedupe_peers,
        dedupe_injected_deps: config.dedupe_injected_deps,
        dedupe_peer_dependents: config.dedupe_peer_dependents,
        resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
        exclude_links_from_lockfile: config.exclude_links_from_lockfile,
        lockfile_dir: lockfile_dir.to_path_buf(),
        peers_suffix_max_length,
        share_workspace_resolutions,
        manifest_hook: manifest_hook.clone(),
        overrides_hook: overrides_hook.clone(),
        pnpmfile_hook,
        read_package_log,
        skipped_optional_log: Some(super::skipped_optional_log_fn::<Reporter>()),
        finalized_package,
        pick_lowest_direct,
        time_based,
        wanted_lockfile: resolution_lockfile,
        reuse_lockfile_subtrees,
        update_reuse_scope,
        update_reuse_scopes_by_importer,
        update_depth,
        auto_install_peers: config.auto_install_peers,
        allowed_deprecated_versions: config.allowed_deprecated_versions.clone(),
        deprecation_log: Some(super::deprecation_log_fn::<Reporter>()),
    };

    pnpm_resolving_deps_resolver::resolve_workspace(
        resolver,
        &workspace_importers,
        dependency_groups,
        workspace_opts,
        |importer| {
            let importer_preferred_versions = preferred_versions_seeds_by_importer
                .get(&importer.id)
                .unwrap_or(preferred_versions_seed);
            let project_dir = importer
                .manifest
                .path()
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf();
            let importer_modules_dir = project_dir.join(&modules_basename);
            ResolveImporterOptions {
                auto_install_peers: config.auto_install_peers,
                auto_install_peers_from_highest_match: config.auto_install_peers_from_highest_match,
                resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
                dedupe_peers: config.dedupe_peers,
                dedupe_peer_dependents: config.dedupe_peer_dependents,
                all_preferred_versions: Arc::clone(importer_preferred_versions),
                override_bare_specifier: override_bare_specifier.clone(),
                patched_dependencies: patched_dependencies.clone(),
                // `resolve_workspace` computes the workspace-wide
                // time-based cutoff and overrides both of these per
                // importer; the values here only satisfy the struct.
                pick_lowest_direct,
                subdep_published_by: published_by,
                base_opts: shared_resolve_options
                    .build(project_dir, Arc::clone(importer_preferred_versions)),
                catalogs: catalogs.clone(),
                exclude_links_from_lockfile: config.exclude_links_from_lockfile,
                lockfile_dir: Some(lockfile_dir.to_path_buf()),
                modules_dir: Some(importer_modules_dir),
                peers_suffix_max_length,
                catalog_server: false,
                manifest_hook: manifest_hook.clone(),
                overrides_hook: overrides_hook.clone(),
                pnpmfile_hook: None,
            }
        },
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

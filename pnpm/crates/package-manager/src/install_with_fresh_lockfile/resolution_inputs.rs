//! Inputs the fresh-install resolve pass consumes: the manifest
//! transform chain (`packageExtensions` + `pnpm.overrides`), the
//! preferred-versions tie-break seeds, and the `preResolution` pnpmfile
//! hook.
//!
//! Split out of [`super::InstallWithFreshLockfile::run`] so the
//! orchestrator reads as a sequence of install phases. Everything here
//! runs between the resolver chain's construction and
//! `resolve_workspace`.

use super::{
    ImporterUpdateSeedPolicy, InstallWithFreshLockfileError, UpdateSeedPolicy,
    compose_manifest_hooks, parse_config_overrides, resolved_overrides_map,
};
use crate::VersionsOverrider;
use indexmap::IndexMap;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::LogLevel;
use pacquet_resolving_deps_resolver::{DependencyOverrider, ManifestHook};
use pacquet_resolving_resolver_base::{PreferredVersions, ResolveOptions};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

/// pnpm's built-in read-package hook chain for the manifests fresh
/// resolution consumes, plus the pieces later phases read off it.
///
/// The order matches `createReadPackageHook`: packageExtensions first,
/// overrides after. The two halves stay separate hooks because the
/// resolver interleaves the pnpmfile's `readPackage` between them —
/// packageExtensions → readPackage → overrides — so a hook that replaces
/// the manifest cannot erase the overrides.
pub(super) struct ManifestTransforms {
    pub parsed_overrides: Option<Vec<pacquet_config_parse_overrides::VersionOverride>>,
    pub resolved_overrides: Option<IndexMap<String, String>>,
    pub package_extensions_checksum: Option<String>,
    pub versions_overrider: Option<Arc<VersionsOverrider>>,
    pub manifest_hook: Option<ManifestHook>,
    pub overrides_hook: Option<ManifestHook>,
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    /// Importer manifests with every transform already applied. Empty
    /// when nothing transforms them, in which case the caller keeps
    /// resolving against the originals.
    pub effective_importer_manifests: BTreeMap<String, PackageManifest>,
}

pub(super) fn build_manifest_transforms(
    config: &Config,
    catalogs: &Catalogs,
    lockfile_dir: &Path,
    importer_manifests: &BTreeMap<String, &PackageManifest>,
) -> Result<ManifestTransforms, InstallWithFreshLockfileError> {
    let parsed_overrides = parse_config_overrides(config, catalogs)?;
    let resolved_overrides = parsed_overrides.as_deref().map(resolved_overrides_map);

    let compat_package_extender = if config.ignore_compatibility_db {
        None
    } else {
        Some(crate::compat_package_extensions::compat_package_extender())
    };
    let package_extender = match config.package_extensions.as_ref() {
        Some(extensions) => {
            let extender = crate::PackageExtender::new(extensions)
                .map_err(InstallWithFreshLockfileError::InvalidPackageExtensionSelector)?;
            (!extender.is_empty()).then(|| Arc::new(extender))
        }
        None => None,
    };
    let package_extensions_checksum = super::compute_package_extensions_checksum(config);
    let versions_overrider = parsed_overrides
        .as_ref()
        .map(|parsed| Arc::new(VersionsOverrider::new(parsed, lockfile_dir)));

    let mut effective_importer_manifests = BTreeMap::new();
    if compat_package_extender.is_some()
        || package_extender.is_some()
        || versions_overrider.as_ref().is_some_and(|overrider| !overrider.is_empty())
    {
        for (id, manifest) in importer_manifests {
            let mut cloned = (*manifest).clone();
            if let Some(extender) = compat_package_extender {
                extender.apply(cloned.value_mut());
            }
            if let Some(extender) = package_extender.as_ref() {
                extender.apply(cloned.value_mut());
            }
            if let Some(overrider) = versions_overrider.as_ref() {
                let manifest_dir = cloned.path().parent().map(Path::to_path_buf);
                overrider.apply(&mut cloned, manifest_dir.as_deref());
            }
            effective_importer_manifests.insert(id.clone(), cloned);
        }
    }

    let compat_package_extensions_hook: Option<ManifestHook> = compat_package_extender
        .map(|extender| Arc::new(move |manifest| extender.apply_to_arc(manifest)) as ManifestHook);
    let package_extensions_hook: Option<ManifestHook> = package_extender.as_ref().map(|extender| {
        let extender = Arc::clone(extender);
        Arc::new(move |manifest| extender.apply_to_arc(manifest)) as ManifestHook
    });
    let overrides_hook: Option<ManifestHook> =
        versions_overrider.as_ref().filter(|overrider| !overrider.is_empty()).map(|overrider| {
            let overrider = Arc::clone(overrider);
            Arc::new(move |manifest| overrider.apply_to_arc(manifest, None)) as ManifestHook
        });
    let override_bare_specifier: Option<Arc<DependencyOverrider>> =
        versions_overrider.as_ref().filter(|overrider| !overrider.is_empty()).map(|overrider| {
            let overrider = Arc::clone(overrider);
            Arc::new(move |name: &str, range: &str, pkg_dir: &Path| {
                overrider.override_for_undeclared_dependency(name, range, pkg_dir)
            }) as Arc<DependencyOverrider>
        });

    Ok(ManifestTransforms {
        parsed_overrides,
        resolved_overrides,
        package_extensions_checksum,
        versions_overrider,
        manifest_hook: compose_manifest_hooks(
            compat_package_extensions_hook,
            package_extensions_hook,
        ),
        overrides_hook,
        override_bare_specifier,
        effective_importer_manifests,
    })
}

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
) -> (Arc<PreferredVersions>, BTreeMap<String, Arc<PreferredVersions>>) {
    use pacquet_lockfile_preferred_versions::{
        get_preferred_versions_from_lockfile_and_manifests as from_lockfile,
        get_preferred_versions_from_lockfile_and_manifests_excluding as from_lockfile_excluding,
    };

    let manifests: Vec<&PackageManifest> = importer_manifests.values().copied().collect();
    let snapshots = wanted_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref());

    let workspace_seed = match update_seed_policy {
        UpdateSeedPolicy::KeepAll
        | UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::ByImporter { .. } => from_lockfile(snapshots, manifests.as_slice()),
        UpdateSeedPolicy::DropAll { .. } => from_lockfile(None, manifests.as_slice()),
        UpdateSeedPolicy::DropOnly { names, .. } => {
            from_lockfile_excluding(snapshots, manifests.as_slice(), &excluded_names(names))
        }
    };

    let mut by_importer = BTreeMap::new();
    if let UpdateSeedPolicy::ByImporter { policies, .. } = update_seed_policy {
        let mut drop_all_seed = None;
        let mut drop_only_seeds = HashMap::new();
        for (importer_id, policy) in policies {
            let seed = match policy {
                ImporterUpdateSeedPolicy::DropAll => Arc::clone(
                    drop_all_seed
                        .get_or_insert_with(|| Arc::new(from_lockfile(None, manifests.as_slice()))),
                ),
                ImporterUpdateSeedPolicy::DropOnly(names) => {
                    let mut cache_key = names.iter().cloned().collect::<Vec<_>>();
                    cache_key.sort_unstable();
                    if let Some(seed) = drop_only_seeds.get(&cache_key) {
                        Arc::clone(seed)
                    } else {
                        let seed = Arc::new(from_lockfile_excluding(
                            snapshots,
                            manifests.as_slice(),
                            &excluded_names(names),
                        ));
                        drop_only_seeds.insert(cache_key, Arc::clone(&seed));
                        seed
                    }
                }
            };
            by_importer.insert(importer_id.clone(), seed);
        }
    }

    (Arc::new(workspace_seed), by_importer)
}

fn excluded_names(
    names: &std::collections::HashSet<String>,
) -> std::collections::HashSet<pacquet_lockfile::PkgName> {
    names.iter().filter_map(|name| pacquet_lockfile::PkgName::parse(name.as_str()).ok()).collect()
}

/// Call the pnpmfile's `preResolution` hook before resolution starts.
pub(super) async fn run_pre_resolution_hook<Reporter: pacquet_reporter::Reporter>(
    hook: &Arc<dyn pacquet_hooks::PnpmfileHooks>,
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
    let ctx = pacquet_hooks::PreResolutionHookContext {
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
        pacquet_hooks::PreResolutionHookLogger {
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
    pub published_by_exclude: Option<pacquet_config::version_policy::PackageVersionPolicy>,
    pub trust_policy: Option<pacquet_config::TrustPolicy>,
    pub trust_policy_exclude: Option<pacquet_config::version_policy::PackageVersionPolicy>,
    pub package_version_guard:
        Option<Arc<dyn pacquet_resolving_resolver_base::PackageVersionGuard>>,
    pub workspace_packages: Option<Arc<pacquet_resolving_resolver_base::WorkspacePackages>>,
    /// See [`super::InstallWithFreshLockfile::update_checksums`].
    pub update_checksums: bool,
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
                != pacquet_config::LinkWorkspacePackages::Off,
            inject_workspace_packages: self.config.inject_workspace_packages,
            prefer_workspace_packages: self.config.prefer_workspace_packages,
            update_checksums: self.update_checksums,
            ..ResolveOptions::default()
        }
    }
}

pub(super) struct ReuseSeedInputs<'a> {
    pub config: &'a Config,
    pub catalogs: &'a Catalogs,
    /// The previous run's lockfile, the only reuse candidate.
    pub wanted_lockfile: Option<&'a Lockfile>,
    pub package_extensions_checksum: Option<&'a str>,
    pub parsed_overrides: Option<&'a [pacquet_config_parse_overrides::VersionOverride]>,
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
    pub npm_resolver: &'a dyn pacquet_resolving_resolver_base::Resolver,
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
/// each yields a dependency-shape-verified seed. Every other shape falls
/// back to withholding.
pub(super) async fn lockfile_reuse_seed(inputs: ReuseSeedInputs<'_>) -> Option<Arc<Lockfile>> {
    use crate::{
        fast_update_catalogs::{FastCatalogUpdate, try_fast_update_catalogs},
        fast_update_overrides::{FastOverrideOptions, try_fast_update_overrides},
    };

    let ReuseSeedInputs {
        config,
        catalogs,
        wanted_lockfile,
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

    let reusable_settings_lockfile = wanted_lockfile.filter(|lockfile| {
        lockfile.package_extensions_checksum.as_deref() == package_extensions_checksum
    });
    let override_settings_match = reusable_settings_lockfile.is_some_and(|lockfile| {
        super::overrides_match(lockfile.overrides.as_ref(), resolved_overrides)
    });

    if let (Some(lockfile), Some(parsed), Some(resolved)) =
        (reusable_settings_lockfile, parsed_overrides, resolved_overrides)
        && catalogs_match
        && !overrides_use_catalogs
        && !override_settings_match
        && fast_override_eligible
        && let Some(seed) = try_fast_update_overrides(FastOverrideOptions {
            lockfile,
            parsed_overrides: parsed,
            resolved_overrides: resolved,
            resolver: npm_resolver,
            resolve_options,
            manifest_hook: super::compose_manifest_hooks(manifest_hook, overrides_hook).as_ref(),
            registries,
            lockfile_include_tarball_url: config.lockfile_include_tarball_url,
        })
        .await
    {
        return Some(Arc::new(seed));
    }

    if override_settings_match
        && reusable_settings_lockfile.is_some()
        && let Some(seed) = fast_catalog_seed
    {
        return Some(Arc::new(seed));
    }

    (catalogs_match && override_settings_match)
        .then_some(reusable_settings_lockfile)
        .flatten()
        .map(|lockfile| Arc::new(lockfile.clone()))
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
pub(super) async fn warn_stale_convergence_overrides<Reporter: pacquet_reporter::Reporter>(
    npm_resolver: &dyn pacquet_resolving_resolver_base::Resolver,
    parsed_overrides: &[pacquet_config_parse_overrides::VersionOverride],
    versions_overrider: &VersionsOverrider,
    lockfile_dir: &Path,
    published_by: Option<chrono::DateTime<chrono::Utc>>,
    published_by_exclude: Option<&pacquet_config::version_policy::PackageVersionPolicy>,
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

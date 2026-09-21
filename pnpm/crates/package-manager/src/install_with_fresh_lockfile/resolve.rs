//! The resolve phase: what the workspace resolution is given, the walk
//! itself, and the diagnostics that read its result.
//!
//! The read-package transform chain the resolve also consumes lives in
//! [`super::manifest_transforms`]; the resolver chain it walks is built
//! by [`super::resolver_setup`].

pub(super) use maturity::resolve_mature_dependency_tree;
pub(super) use reuse::{ReuseSeedInputs, lockfile_reuse_seed, preferred_versions_seeds};

mod maturity;
mod reuse;

use super::InstallWithFreshLockfileError;
use crate::VersionsOverrider;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::LogLevel;
use pnpm_resolving_deps_resolver::{
    DependencyOverrider, ResolveImporterError, ResolveImporterOptions,
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
#[derive(Clone)]
pub(super) struct SharedResolveOptions<'a> {
    pub policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions,
    pub config: &'a Config,
    pub lockfile_dir: &'a Path,
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
            project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                project_dir,
                lockfile_dir: self.lockfile_dir.to_path_buf(),
                workspace_packages: self.workspace_packages.clone(),
                link_workspace_packages: self.config.link_workspace_packages,
                inject_workspace_packages: self.config.inject_workspace_packages,
                prefer_workspace_packages: self.config.prefer_workspace_packages,
            },
            version: pnpm_resolving_resolver_base::VersionSelectionOptions {
                preferred_versions,
                default_tag: Some("latest".to_string()),
                ..Default::default()
            },
            refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
                update_checksums: self.update_checksums,
                update: self.update_behavior,
                ..Default::default()
            },
            policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
                published_by: self.policy.published_by,
                published_by_exclude: self.policy.published_by_exclude.clone(),
                trust_policy: self.policy.trust_policy,
                trust_policy_exclude: self.policy.trust_policy_exclude.clone(),
                trust_policy_ignore_after: self.config.trust_policy_ignore_after,
                package_version_guard: self.policy.package_version_guard.clone(),
                block_exotic_subdeps: self.config.block_exotic_subdeps,
                blocked_versions: self.policy.blocked_versions.clone(),
            },
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
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: lockfile_dir.to_path_buf(),
            lockfile_dir: lockfile_dir.to_path_buf(),
            ..Default::default()
        },
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            default_tag: Some("latest".to_string()),
            ..Default::default()
        },
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by,
            published_by_exclude: published_by_exclude.cloned(),
            ..Default::default()
        },
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
    pub hooks: crate::install_with_fresh_lockfile::resolution_inputs::WorkspaceLifecycleHooks,
    pub reuse: pnpm_resolving_deps_resolver::WorkspaceLockfileReuse,
    /// See
    /// [`WorkspaceResolveOptions::share_workspace_resolutions`](pnpm_resolving_deps_resolver::WorkspaceResolveOptions::share_workspace_resolutions).
    pub share_workspace_resolutions: bool,
    pub time_based: bool,
    pub registries: HashMap<String, String>,
    pub registries_by_prefix: HashMap<String, String>,
}

/// What every importer's resolve reads, and the walk reads alongside.
pub(super) struct ImporterInputs<'a> {
    pub hooks: pnpm_resolving_deps_resolver::ManifestTransformHooks,
    pub versions: crate::install_with_fresh_lockfile::resolution_inputs::ImporterVersionSeeds<'a>,
    pub config: &'a Config,
    pub catalogs: &'a Catalogs,
    pub lockfile_dir: &'a Path,
    /// The `ResolveOptions` half every importer shares; the per-importer
    /// half is its own `project_dir` and preferred-versions seed.
    pub shared_resolve_options: &'a SharedResolveOptions<'a>,
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    pub patched_dependencies: Option<Arc<pnpm_patching::PatchGroupRecord>>,
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
        let preferred_versions = self.versions.for_importer(&importer.id);
        let project_dir = importer.manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
            .to_path_buf();
        let modules_dir = Some(project_dir.join(modules_basename));
        ResolveImporterOptions {
            base_opts: self.shared_resolve_options.build(
                project_dir,
                Arc::clone(preferred_versions),
            ),
            peers_suffix_max_length: self.peers_suffix_max_length(),
            peers: pnpm_resolving_deps_resolver::ImporterPeerOptions {
                auto_install_peers: self.config.auto_install_peers,
                auto_install_peers_from_highest_match: self.config
                    .auto_install_peers_from_highest_match,
                resolve_peers_from_workspace_root: self.config.resolve_peers_from_workspace_root,
                dedupe_peers: self.config.dedupe_peers,
                dedupe_peer_dependents: self.config.dedupe_peer_dependents,
            },
            links: pnpm_resolving_deps_resolver::PeerLinkOptions {
                modules_dir,
                exclude_links_from_lockfile: self.config.exclude_links_from_lockfile,
                lockfile_dir: Some(self.lockfile_dir.to_path_buf()),
            },
            resolution: pnpm_resolving_deps_resolver::ImporterResolutionInputs {
                all_preferred_versions: Arc::clone(preferred_versions),
                override_bare_specifier: self.override_bare_specifier.clone(),
                patched_dependencies: self.patched_dependencies.clone(),
                // `resolve_workspace` computes the workspace-wide
                // time-based cutoff and overrides both of these per
                // importer; the values here only satisfy the struct.
                pick_lowest_direct: self.versions.pick_lowest,
                subdep_published_by: self.versions.published_by,
                catalogs: self.catalogs.clone(),
                catalogs_dir: self.config.workspace_dir.clone(),
                catalog_server: false,
            },
            hooks: self.hooks.clone(),
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
            share_workspace_resolutions: self.share_workspace_resolutions,
            allowed_deprecated_versions: config.allowed_deprecated_versions.clone(),
            peers: pnpm_resolving_deps_resolver::WorkspacePeerResolutionOptions {
                dedupe_peers: config.dedupe_peers,
                dedupe_injected_deps: config.dedupe_injected_deps,
                dedupe_peer_dependents: config.dedupe_peer_dependents,
                resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
                exclude_links_from_lockfile: config.exclude_links_from_lockfile,
                lockfile_dir: shared.lockfile_dir.to_path_buf(),
                peers_suffix_max_length: shared.peers_suffix_max_length(),
                auto_install_peers: config.auto_install_peers,
            },
            hooks: pnpm_resolving_deps_resolver::WorkspaceResolveHooks {
                read_package_log: self.hooks.read_package_log,
                skipped_optional_log: Some(super::skipped_optional_log_fn::<Reporter>()),
                finalized_package: self.hooks.finalized_package,
                deprecation_log: Some(super::deprecation_log_fn::<Reporter>()),
                manifests: pnpm_resolving_deps_resolver::ManifestTransformHooks {
                    manifest_hook: shared.hooks.manifest_hook.clone(),
                    overrides_hook: shared.hooks.overrides_hook.clone(),
                    pnpmfile_hook: self.hooks.pnpmfile,
                },
            },
            reuse: self.reuse,
            version: pnpm_resolving_deps_resolver::WorkspaceVersionResolution {
                pick_lowest_direct: shared.versions.pick_lowest,
                time_based: self.time_based,
            },
        }
    }
}

pub(super) async fn run_dependency_pass<Reporter: pnpm_reporter::Reporter>(
    inputs: ResolvePassInputs<'_>,
) -> Result<
    pnpm_resolving_deps_resolver::ResolvedWorkspaceDependencies,
    InstallWithFreshLockfileError,
> {
    let ResolvePassInputs {
        resolver,
        importer_manifests,
        dependency_groups,
        walk,
        per_importer,
    } = inputs;
    let workspace_importers: Vec<pnpm_resolving_deps_resolver::WorkspaceImporter<'_>> =
        importer_manifests
            .iter()
            .map(|(id, manifest)| pnpm_resolving_deps_resolver::WorkspaceImporter {
                id: id.clone(),
                manifest,
            })
            .collect();
    let modules_basename = per_importer.config.modules_dir
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("node_modules"), std::ffi::OsStr::to_os_string);
    pnpm_resolving_deps_resolver::resolve_workspace_dependencies(
        resolver,
        &workspace_importers,
        dependency_groups,
        walk.into_options::<Reporter>(&per_importer),
        |importer| per_importer.resolve_importer_options(importer, &modules_basename),
    )
    .await
    .map_err(resolve_error)
}

pub(super) fn resolve_error(err: ResolveImporterError) -> InstallWithFreshLockfileError {
    match err {
        ResolveImporterError::Resolve(err) => {
            InstallWithFreshLockfileError::ResolveDependencyTree(err)
        }
        ResolveImporterError::RootDepManifest(err) => {
            InstallWithFreshLockfileError::RootDepManifest(err)
        }
    }
}

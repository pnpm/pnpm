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
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_resolving_deps_resolver::{
    DependencyOverrider, ResolveImporterError, ResolveImporterOptions,
};
use pnpm_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pnpm_resolving_resolver_base::{
    BlockedVersions, PreferredVersions, ResolutionPolicyViolation, ResolveOptions, Resolver,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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

/// Upper bound on resolution passes.
///
/// The loop already terminates on its own — every pass blocks at least one
/// more version, over a finite set — but "finite" is not "small": a package
/// whose every version in range pins something too young would be walked one
/// version per pass, and each pass is a full tree resolution. The bound is
/// what stops that from running for minutes.
///
/// It is set well above the depth any real dependency chain reaches, since
/// blame only climbs one ancestor per pass and a tree deep enough to need more
/// has an unusual number of consecutive exact pins. Hitting it is reported
/// rather than passed over silently — the install then answers with the first
/// pass, and the user has no other way to tell that a later attempt might have
/// found a tree.
const MAX_RESOLUTION_PASSES: usize = 32;

/// Resolve the workspace, backing out of subtrees that no
/// `minimumReleaseAge` cutoff can satisfy.
///
/// The cutoff narrows candidates one packument at a time, so an edge that
/// admits no mature version is a dead end the pick itself cannot escape: a
/// parent that pins its platform bindings to a version whose release was not
/// atomic, or whose newest release depends on a package published minutes
/// ago, has no mature answer to offer. The way out is to pick a different
/// version of whatever declared that edge, and the resolver only reconsiders
/// that on a fresh pass.
///
/// So each pass blocks the immediate parent of every immature pick and
/// resolves again, walking the blame one level up per pass until a tree comes
/// back clean. Passes after the first fetch no registry metadata — the
/// packuments are memoized for the install — and only run while every
/// immature pick still has a parent whose choice could be revisited, so an
/// install that resolves cleanly, or one whose immature picks the manifests
/// ask for by name, pays nothing.
///
/// When no pass comes back clean, the first pass's result is returned: an
/// unavoidable conflict has to report the versions the manifests actually
/// resolve to, not whatever the last attempt happened to reach.
pub(super) async fn resolve_mature_dependency_tree<Reporter, Resolve, Fut>(
    mut resolve: Resolve,
    lockfile_dir: &Path,
    minimum_release_age_active: bool,
) -> Result<pnpm_resolving_deps_resolver::ResolveWorkspaceResult, InstallWithFreshLockfileError>
where
    Reporter: pnpm_reporter::Reporter,
    Resolve: FnMut(Option<Arc<BlockedVersions>>) -> Fut,
    Fut: std::future::Future<Output = Result<pnpm_resolving_deps_resolver::ResolveWorkspaceResult, InstallWithFreshLockfileError>>,
{
    let first_pass = resolve(None).await?;
    if !minimum_release_age_active || first_pass.merged_tree.policy_violations.is_empty() {
        return Ok(first_pass);
    }

    let mut blocked_versions: BlockedVersions = HashMap::new();
    let mut last_pass: Option<pnpm_resolving_deps_resolver::ResolveWorkspaceResult> = None;
    for _ in 1..MAX_RESOLUTION_PASSES {
        let source = last_pass.as_ref().unwrap_or(&first_pass);
        if !block_dead_end_parents(&source.merged_tree.policy_violations, &mut blocked_versions) {
            return Ok(first_pass);
        }
        let pass = resolve(Some(Arc::new(blocked_versions.clone()))).await?;
        if pass.merged_tree.policy_violations.is_empty() {
            report_held_back_parents::<Reporter>(&blocked_versions, &pass, lockfile_dir);
            return Ok(pass);
        }
        last_pass = Some(pass);
    }
    // Fell out of the loop with ancestors still left to try, so the report
    // below is the first pass's, not a proof that no installable tree exists.
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        prefix: inputs.lockfile_dir.display().to_string(),
        message: format!(
            "Stopped after {} resolution attempts while backing off from versions whose \
             dependencies do not satisfy minimumReleaseAge. The versions reported are the ones \
             the first attempt resolved to; an installable combination may still exist further \
             down their ranges.",
            MAX_RESOLUTION_PASSES,
        ),
    }));
    Ok(first_pass)
}

/// Record the immediate parent of every immature pick as unusable, and
/// report whether another pass could still reach a clean tree.
///
/// It cannot when a violation has no parent to blame: the importer named
/// that package itself, and no ancestor's choice can widen a range the
/// manifest fixes. Retrying past one of those only re-reaches the same
/// failure, so the install stops here and lets the policy handler act on
/// this pass. It cannot either when every parent to blame is already
/// blocked, which means the walk has run out of ancestors to move.
fn block_dead_end_parents(
    violations: &[ResolutionPolicyViolation],
    blocked_versions: &mut BlockedVersions,
) -> bool {
    let mut grew = false;
    for violation in violations {
        if violation.code != MINIMUM_RELEASE_AGE_VIOLATION_CODE {
            continue;
        }
        let Some(parent) = violation.parents.last() else { return false };
        grew |= blocked_versions
            .entry(parent.name.to_string())
            .or_default()
            .insert(parent.suffix.to_string());
    }
    grew
}

fn report_held_back_parents<Reporter: pnpm_reporter::Reporter>(
    blocked_versions: &BlockedVersions,
    resolved: &pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    lockfile_dir: &Path,
) {
    let mut resolved_versions_by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for pkg in resolved.merged_tree.packages.values() {
        if let Some(name_ver) = pkg.result.package.name_ver.as_ref() {
            resolved_versions_by_name
                .entry(name_ver.name.to_string())
                .or_default()
                .insert(name_ver.suffix.to_string());
        }
    }
    // Sorted so the report reads the same across runs: the blocked-version
    // set is keyed by hash, and its iteration order is not stable between them.
    let mut blocked_by_name: Vec<(&String, &HashSet<String>)> = blocked_versions.iter().collect();
    blocked_by_name.sort_by(|left, right| left.0.cmp(right.0));
    let mut lines = Vec::new();
    for (name, versions) in blocked_by_name {
        let resolved_to = resolved_versions_by_name
            .get(name)
            .map(|versions| versions.iter().cloned().collect::<Vec<_>>().join(", "));
        let mut sorted_versions: Vec<&String> = versions.iter().collect();
        sorted_versions.sort();
        for version in sorted_versions {
            match resolved_to.as_deref() {
                Some(resolved_to) => {
                    lines.push(format!("  {name}@{version} (resolved to {resolved_to} instead)"));
                }
                None => lines.push(format!("  {name}@{version}")),
            }
        }
    }
    if lines.is_empty() {
        return;
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        prefix: lockfile_dir.display().to_string(),
        message: format!(
            "minimumReleaseAge held back the following versions because a package they \
             depend on is younger than the cutoff:\n{}",
            lines.join("\n"),
        ),
    }));
}

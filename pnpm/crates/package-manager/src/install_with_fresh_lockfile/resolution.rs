pub(super) use early_materializer::{
    FastOverrideFit, fast_override_eligible, interactive_policy, start_early_materialization,
};

mod completion;
mod early_materializer;
use completion::collect_resolution;

use super::{
    FreshInputs, OwnedInputs, PatchUsageScope, check_patch_usage,
    errors::InstallWithFreshLockfileError,
    importers_consuming_linked_peers, manifest_transforms, report_peer_issues, resolve,
    resolver_setup,
    seed_policy::full_resolution_required,
    setup::{ResolutionPrep, ResolverSetup, prepare_resolution, resolver_update_behavior},
};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

/// What the resolve phase leaves for the lockfile and the on-disk phases.
///
/// The importer manifests view borrows the slots `run` owns; the repaired
/// wanted lockfile under `fix-lockfile` is owned here and `run` derives
/// its view after the phase returns.
pub(super) struct Resolved<'a, Reporter> {
    pub(super) early_materializer:
        Option<Arc<crate::early_materializer::EarlyMaterializer<Reporter>>>,
    pub(super) importer_manifests: ManifestsView<'a>,
    pub(super) fixed_wanted_lockfile: Option<Lockfile>,
    pub(super) overrides: ResolvedOverrides,
    pub(super) patches: super::setup::Patches,
    pub(super) hooks: ResolvedHooks,
    pub(super) reuse: ResolutionReuseGuard<'a>,
    pub(super) graph: ResolvedGraph,
}

pub(super) struct ResolvedOverrides {
    pub(super) parsed_overrides: Option<Vec<pnpm_config_parse_overrides::VersionOverride>>,
    pub(super) overrides: Option<IndexMap<String, String>>,
    pub(super) versions_overrider: Option<Arc<crate::VersionsOverrider>>,
}

pub(super) struct ResolvedHooks {
    pub(super) after_all_resolved_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) after_all_resolved_log: Option<pnpm_hooks::LogFn>,
}

pub(super) struct ResolutionReuseGuard<'a> {
    pub(super) guard_previous_importers:
        Option<&'a HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    pub(super) guard_update_reuse_scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    pub(super) guard_update_reuse_scopes_by_importer:
        BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
    pub(super) full_resolution: bool,
}

pub(super) struct ResolvedGraph {
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) merged_graph: pnpm_resolving_deps_resolver::DependenciesGraph,
    pub(super) direct_by_importer: BTreeMap<String, BTreeMap<String, pnpm_deps_path::DepPath>>,
    pub(super) time: BTreeMap<String, String>,
}

/// The importer manifests as declared, and as the transforms rewrote them
/// when any applied. `run` owns them so the resolve phase can hand back
/// one view that every later phase reads.
pub(super) struct ManifestSlots<'a> {
    declared: BTreeMap<String, &'a PackageManifest>,
    effective: BTreeMap<String, PackageManifest>,
}
/// The importer manifests as the resolver sees them: the transforms'
/// rewrites when they made any, the declared manifests otherwise.
pub(super) type ManifestsView<'m> = std::borrow::Cow<'m, BTreeMap<String, &'m PackageManifest>>;
impl<'a> ManifestSlots<'a> {
    pub(super) fn declared(declared: BTreeMap<String, &'a PackageManifest>) -> Self {
        Self { declared, effective: BTreeMap::new() }
    }

    /// Build the read-package hook chain and rewrite every importer's
    /// manifest through it.
    pub(super) fn transform(
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
                self.effective
                    .iter()
                    .map(|(id, manifest)| (id.clone(), manifest))
                    .collect(),
            )
        }
    }
}
/// Resolve every importer's dependency graph. Consumes the registries,
/// the pnpmfile hook and the shared wanted lockfile.
pub(super) async fn resolve_graph<'a: 'm, 'm, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    owned: &mut OwnedInputs,
    setup: &mut ResolverSetup,
    manifests: &'m mut ManifestSlots<'a>,
) -> Result<Resolved<'m, Reporter>, InstallWithFreshLockfileError> {
    let registries = std::mem::take(&mut setup.registries);
    let prep = prepare_resolution::<Reporter>(install, owned, setup, manifests).await?;
    let importer_manifests = manifests.view();
    let pass = run_prepared_resolve(
        ResolutionContext { install, owned, setup, prep: &prep, registries },
        importer_manifests,
    )
    .await?;
    collect_resolution::<Reporter>(install, owned.resolution.peer_issues_sink.as_ref(), prep, pass)
        .await
}
pub(super) struct ResolutionContext<'a, Reporter> {
    install: FreshInputs<'a>,
    owned: &'a OwnedInputs,
    setup: &'a ResolverSetup,
    prep: &'a ResolutionPrep<Reporter>,
    registries: resolver_setup::Registries,
}
impl<'a, Reporter: self::Reporter + 'static> ResolutionContext<'a, Reporter> {
    fn wanted_lockfile(&self) -> Option<&Lockfile> {
        self.prep.fixed_wanted_lockfile.as_ref().or(self.install.lockfiles.wanted)
    }

    fn shared_options(&self) -> resolve::SharedResolveOptions<'a> {
        resolve::SharedResolveOptions {
            policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
                published_by: self.setup.policy.published_by,
                published_by_exclude: self.setup.policy.published_by_exclude.clone(),
                trust_policy: self.prep.trust.policy,
                trust_policy_exclude: self.prep.trust.exclude.clone(),
                package_version_guard: self.setup.observer.package_version_guard.clone(),
                ..Default::default()
            },
            config: self.install.drivers.config,
            lockfile_dir: self.install.projects.lockfile_dir,

            workspace_packages: self.setup.workspace_packages.clone(),
            update_checksums: self.install.update_checksums,
            update_behavior: resolver_update_behavior(&self.owned.resolution.update_seed_policy),
        }
    }

    async fn reuse_seed(
        &self,
        shared_resolve_options: &resolve::SharedResolveOptions<'_>,
        preferred_versions_seed: &Arc<pnpm_resolving_resolver_base::PreferredVersions>,
    ) -> Option<Arc<Lockfile>> {
        resolve::lockfile_reuse_seed(resolve::ReuseSeedInputs {
            hooks: pnpm_resolving_deps_resolver::ManifestTransformHooks {
                manifest_hook: self.prep.transforms.hooks.manifest_hook.clone(),
                overrides_hook: self.prep.transforms.hooks.overrides_hook.clone(),
                pnpmfile_hook: None,
            },
            lockfile: crate::install_with_fresh_lockfile::resolution_inputs::ReuseLockfileInputs {
                wanted: self.wanted_lockfile(),
                shared: self.prep.wanted_lockfile_shared.as_ref(),
                extensions_checksum: self.prep.transforms
                    .package_extensions_checksum
                    .as_deref(),
                parsed_overrides: self.prep.transforms.parsed_overrides.as_deref(),
                resolved_overrides: self.prep.transforms.resolved_overrides.as_ref(),
            },
            config: self.install.drivers.config,
            catalogs: &self.owned.projects.catalogs,

            fast_override_eligible: fast_override_eligible(FastOverrideFit {
                has_pnpmfile_hook: self.prep.hooks.pnpmfile_hook.is_some(),
                has_custom_resolvers: !self.setup.chain.custom_resolvers.is_empty(),
                has_patches: self.prep.patches.record.is_some(),
                can_fast_update_overrides: self.setup.observer.can_fast_update_overrides,
            }),
            npm_resolver: &*self.setup.chain.npm_resolver,
            resolve_options: &shared_resolve_options.build(
                self.install.projects.lockfile_dir.to_path_buf(),
                Arc::clone(preferred_versions_seed),
            ),
            registries: &self.registries.by_scope,
        })
        .await
    }

    fn workspace_walk(
        &mut self,
        lockfile_reuse_seed: Option<&Arc<Lockfile>>,
    ) -> resolve::WorkspaceWalk {
        resolve::WorkspaceWalk {
            hooks: crate::install_with_fresh_lockfile::resolution_inputs::WorkspaceLifecycleHooks {
                pnpmfile: self.prep.hooks.pnpmfile_hook.clone(),
                read_package_log: self.prep.hooks.read_package_log.clone(),
                finalized_package: self.prep.early_materializer
                    .as_ref()
                    .map(crate::early_materializer::EarlyMaterializer::hook),
            },
            reuse: pnpm_resolving_deps_resolver::WorkspaceLockfileReuse {
                lockfile: lockfile_reuse_seed
                    .cloned()
                    .or_else(|| self.prep.wanted_lockfile_shared.clone())
                    .or_else(|| {
                        self.wanted_lockfile()
                            .cloned()
                            .map(Arc::new)
                    }),
                subtrees: lockfile_reuse_seed.is_some(),
                scope: self.prep.reuse.scope.clone(),
                scopes_by_importer: self.prep.reuse.by_importer.clone(),
                depth: self.owned.resolution.update_seed_policy.max_depth(),
            },
            share_workspace_resolutions: self.setup.chain.custom_resolvers.is_empty(),

            time_based: self.setup.policy.time_based,

            registries_by_prefix: self.registries.named.clone(),
            registries: std::mem::take(&mut self.registries.by_scope),
        }
    }

    fn completed_pass<'m>(
        &self,
        workspace_result: pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
        has_reusable_seed: bool,
        phase_start: std::time::Instant,
        importer_manifests: ManifestsView<'m>,
    ) -> ResolvePass<'m> {
        ResolvePass {
            result: workspace_result,
            full_resolution: full_resolution_required(
                has_reusable_seed,
                importer_manifests.keys().map(String::as_str),
                &self.prep.reuse.scope,
                &self.prep.reuse.by_importer,
            ),
            started: phase_start,
            linked_peer_importers: importers_consuming_linked_peers(
                &importer_manifests,
                self.install.projects.lockfile_dir,
            ),
            importer_manifests,
        }
    }

    fn importer_inputs<'b>(
        &'b self,
        shared_resolve_options: &'b resolve::SharedResolveOptions<'b>,
        preferred_versions_seed: &'b Arc<pnpm_resolving_resolver_base::PreferredVersions>,
        preferred_versions_seeds_by_importer: &'b BTreeMap<
            String,
            Arc<pnpm_resolving_resolver_base::PreferredVersions>,
        >,
    ) -> resolve::ImporterInputs<'b> {
        resolve::ImporterInputs {
            hooks: pnpm_resolving_deps_resolver::ManifestTransformHooks {
                manifest_hook: self.prep.transforms.hooks.manifest_hook.clone(),
                overrides_hook: self.prep.transforms.hooks.overrides_hook.clone(),
                pnpmfile_hook: None,
            },
            versions: crate::install_with_fresh_lockfile::resolution_inputs::ImporterVersionSeeds {
                shared: preferred_versions_seed,
                by_importer: preferred_versions_seeds_by_importer,
                pick_lowest: self.setup.policy.pick_lowest_direct,
                published_by: self.setup.policy.published_by,
            },
            config: self.install.drivers.config,
            catalogs: &self.owned.projects.catalogs,
            lockfile_dir: self.install.projects.lockfile_dir,
            shared_resolve_options,

            override_bare_specifier: self.prep.transforms.hooks
                .override_bare_specifier
                .clone(),
            patched_dependencies: self.prep.patches.record.clone(),
        }
    }
}
pub(super) async fn run_prepared_resolve<'m, Reporter: self::Reporter + 'static>(
    mut context: ResolutionContext<'_, Reporter>,
    importer_manifests: ManifestsView<'m>,
) -> Result<ResolvePass<'m>, InstallWithFreshLockfileError> {
    let wanted_lockfile = context.wanted_lockfile();
    let (preferred_versions_seed, preferred_versions_seeds_by_importer) =
        resolve::preferred_versions_seeds(
            &context.owned.resolution.update_seed_policy,
            wanted_lockfile,
            &importer_manifests,
            context.owned.resolution.preferred_versions_override.as_ref(),
        );
    let shared_resolve_options = context.shared_options();
    let lockfile_reuse_seed =
        context.reuse_seed(&shared_resolve_options, &preferred_versions_seed).await;
    let phase_start = std::time::Instant::now();
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: context.install.projects.lockfile_dir.display().to_string(),
        stage: Stage::ResolutionStarted,
    }));
    let walk = context.workspace_walk(lockfile_reuse_seed.as_ref());
    let workspace_result = resolve::run_resolve_pass::<Reporter>(resolve::ResolvePassInputs {
        resolver: &*context.setup.chain.resolver,
        importer_manifests: &importer_manifests,
        dependency_groups: if context.install.execution.save_lockfile {
            &crate::DIRECT_GROUPS
        } else {
            context.install.projects.dependency_groups
        },
        walk,
        per_importer: context.importer_inputs(
            &shared_resolve_options,
            &preferred_versions_seed,
            &preferred_versions_seeds_by_importer,
        ),
    })
    .await?;
    Ok(context.completed_pass(
        workspace_result,
        lockfile_reuse_seed.is_some(),
        phase_start,
        importer_manifests,
    ))
}

pub(super) struct ResolvePass<'m> {
    result: pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    full_resolution: bool,
    started: std::time::Instant,
    /// Importers whose linked packages consume peers.
    linked_peer_importers: HashSet<String>,
    importer_manifests: ManifestsView<'m>,
}

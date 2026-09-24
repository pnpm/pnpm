use super::{
    FreshInputs, errors::InstallWithFreshLockfileError, on_disk::await_lockfile_gate,
    plan::LockfileViews, resolution::Resolved, setup::pnpmfile_checksum, verify_repair_if_filtered,
};
use crate::{
    DependenciesGraphToLockfileError, GraphToLockfileOptions, ImporterLockfileInput,
    dependencies_graph_to_lockfile,
};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::ManifestHook;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

/// Build the wanted lockfile from the resolved graph and verify a
/// filtered repair against the registry.
///
/// The lockfile verification gate is awaited before the build under
/// `--lockfile-only`, and only for a filtered repair otherwise, so a
/// full install's build overlaps the verification. A materializing
/// install builds the lockfile all the same: the layout and the bin-link
/// pass read its `snapshots:` and `packages:` maps, it costs ~3 ms on the
/// alotta-files fixture, and it is what gets saved anyway.
pub(super) async fn build_lockfile_phase<'a, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    lockfile_verification_gate: &mut Option<crate::LockfileVerificationGate>,
    resolved_time: BTreeMap<String, String>,
    resolved: &Resolved<'a, Reporter>,
    views: LockfileViews<'_, 'a>,
    verify_filtered_repair: bool,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    let pnpmfile_checksum = if install.drivers.config.ignore_pnpmfile {
        install.lockfiles.wanted
            .or(install.lockfiles.merge_wanted)
            .and_then(|lockfile| lockfile.pnpmfile_checksum.clone())
    } else {
        pnpmfile_checksum(resolved.hooks.after_all_resolved_hook.as_ref()).await
    };
    let untracked_pnpmfile_read_package_hook =
        pnpm_hooks::untracked_read_package_hook(resolved.hooks.after_all_resolved_hook.as_ref())
            .await
            .map_err(InstallWithFreshLockfileError::PnpmfileHook)?;
    if install.execution.lockfile_only {
        await_lockfile_gate(lockfile_verification_gate).await?;
    }
    let phase_start = std::time::Instant::now();
    let built_lockfile = build_resolved_lockfile(
        install,
        resolved,
        views,
        resolved_time,
        pnpmfile_checksum.as_deref(),
        untracked_pnpmfile_read_package_hook,
    )?;
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
pub(super) fn build_resolved_lockfile<Reporter>(
    install: FreshInputs<'_>,
    resolved: &Resolved<'_, Reporter>,
    views: LockfileViews<'_, '_>,
    resolved_time: BTreeMap<String, String>,
    pnpmfile_checksum: Option<&str>,
    untracked_pnpmfile_read_package_hook: Option<bool>,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    build_lockfile(FreshLockfileBuildOptions {
        inputs: FreshLockfileInputs {
            prior: crate::install_with_fresh_lockfile::resolution_inputs::FreshLockfilePrior {
                importers: resolved.reuse.guard_previous_importers,
                scope: resolved.reuse.guard_update_reuse_scope.clone(),
                scopes_by_importer: resolved.reuse
                    .guard_update_reuse_scopes_by_importer
                    .clone(),
                lockfile: views.wanted_lockfile,
            },
            resolution:
                crate::install_with_fresh_lockfile::resolution_inputs::FreshLockfileResolution {
                    graph: &resolved.graph.merged_graph,
                    direct_by_importer: &resolved.graph.direct_by_importer,
                    overrides: resolved.overrides.overrides.clone(),
                    include_peer_dependencies: resolves_peer_dependencies(&install),
                    time: resolved_time,
                },
            config: install.drivers.config,
            importer_manifests: views.importer_manifests,
            lockfile_specifier_manifests: views.lockfile_specifier_manifests,

            catalogs: views.catalogs,
            manifest_settings: FreshLockfileManifestSettings {
                pnpmfile_checksum,
                untracked_pnpmfile_read_package_hook,
                patched_dependency_hashes: resolved.patches.hashes.as_ref(),
            },
        },
        splice: FilteredSplice {
            merge_wanted_lockfile: install.lockfiles.merge_wanted,
            real_importer_ids: install.projects.real_ids,
            selected_importer_ids: install.projects.selected_ids,
            lockfile_dir: install.projects.lockfile_dir,
        },
        bumps: SpecBumps {
            manifest_spec_bumps: install.manifests.spec_bumps,
            versions_overrider: resolved.overrides.versions_overrider.as_deref(),
        },
    })
}
fn resolves_peer_dependencies(install: &FreshInputs<'_>) -> bool {
    install.resolved_groups().contains(&pnpm_package_manifest::DependencyGroup::Peer)
}
pub(super) fn parse_config_overrides(
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
pub(super) fn resolved_overrides_map(
    parsed: &[pnpm_config_parse_overrides::VersionOverride],
) -> IndexMap<String, String> {
    parsed
        .iter()
        .map(|entry| (entry.selector.clone(), entry.new_bare_specifier.clone()))
        .collect()
}
pub(super) fn overrides_match(
    lockfile: Option<&IndexMap<String, String>>,
    config: Option<&IndexMap<String, String>>,
) -> bool {
    let lockfile = lockfile.filter(|map| !map.is_empty());
    let config = config.filter(|map| !map.is_empty());
    match (lockfile, config) {
        (None, None) => true,
        (Some(lockfile), Some(config)) => {
            lockfile.len() == config.len()
                && lockfile
                    .iter()
                    .all(|(key, value)| {
                        config
                            .get(key)
                            .is_some_and(|config_value| config_value == value)
                    })
        }
        _ => false,
    }
}
pub(super) fn ignored_optional_dependencies_match(
    left: Option<&[String]>,
    right: Option<&[String]>,
) -> bool {
    let left: HashSet<_> = left
        .unwrap_or_default()
        .iter()
        .collect();
    let right: HashSet<_> = right
        .unwrap_or_default()
        .iter()
        .collect();
    left == right
}
pub(super) fn compose_manifest_hooks(
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
pub(super) struct FreshLockfileBuildOptions<'a> {
    inputs: FreshLockfileInputs<'a>,
    splice: FilteredSplice<'a>,
    bumps: SpecBumps<'a>,
}
/// What [`dependencies_graph_to_lockfile()`] lifts to the wire shape.
pub(super) struct FreshLockfileInputs<'a> {
    pub prior: crate::install_with_fresh_lockfile::resolution_inputs::FreshLockfilePrior<'a>,
    pub resolution:
        crate::install_with_fresh_lockfile::resolution_inputs::FreshLockfileResolution<'a>,
    config: &'a Config,
    importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    lockfile_specifier_manifests: Option<&'a BTreeMap<String, PackageManifest>>,
    catalogs: &'a pnpm_catalogs_types::Catalogs,
    manifest_settings: FreshLockfileManifestSettings<'a>,
}
struct FreshLockfileManifestSettings<'a> {
    pnpmfile_checksum: Option<&'a str>,
    untracked_pnpmfile_read_package_hook: Option<bool>,
    patched_dependency_hashes: Option<&'a BTreeMap<String, String>>,
}
impl FreshLockfileManifestSettings<'_> {
    fn build(
        self,
        config: &Config,
        overrides: Option<indexmap::IndexMap<String, String>>,
        include_peer_dependencies: bool,
    ) -> crate::LockfileManifestSettings {
        crate::LockfileManifestSettings {
            include_peer_dependencies,
            overrides,
            ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
            patched_dependencies: self.patched_dependency_hashes.cloned(),
            package_extensions_checksum: compute_package_extensions_checksum(config),
            pnpmfile_checksum: self.pnpmfile_checksum.map(str::to_string),
            untracked_pnpmfile_read_package_hook: self.untracked_pnpmfile_read_package_hook,
        }
    }
}
/// The previous run's lockfile, spliced back over the importers a
/// filtered install did not resolve.
pub(super) struct FilteredSplice<'a> {
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
/// See [`super::FreshManifestOptions::spec_bumps`].
pub(super) struct SpecBumps<'a> {
    manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    /// The override set the run resolved under. Consulted only alongside
    /// [`Self::manifest_spec_bumps`], to leave a declaration an override
    /// governs where the project wrote it.
    versions_overrider: Option<&'a crate::VersionsOverrider>,
}
impl SpecBumps<'_> {
    fn apply(self, built: &mut Lockfile, importer_manifests: &BTreeMap<String, &PackageManifest>) {
        let Some(bumps) = self.manifest_spec_bumps else { return };
        let overridden = self.versions_overrider
            .filter(|overrider| !overrider.is_empty())
            .map(|overrider| crate::manifest_spec_bumps::OverriddenDeclarations {
                overrider,
                importer_manifests,
            });
        crate::manifest_spec_bumps::apply_manifest_spec_bumps(built, bumps, overridden.as_ref());
    }
}
pub(super) fn build_lockfile(
    opts: FreshLockfileBuildOptions<'_>,
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    let FreshLockfileBuildOptions { inputs, splice, bumps } = opts;
    let importer_manifests = inputs.importer_manifests;
    let selected_importer_ids = splice.selected_importer_ids;
    let prune_explicit_peers =
        !inputs.config.auto_install_peers && inputs.resolution.include_peer_dependencies;
    let freshly_resolved = build_fresh_lockfile(inputs)
        .map_err(|error| {
            InstallWithFreshLockfileError::DependenciesGraphToLockfile(Box::new(error))
        })?;
    let mut built = splice.apply(freshly_resolved)?;
    bumps.apply(&mut built, importer_manifests);
    prune_uninstalled_explicit_peers(
        &mut built,
        importer_manifests,
        selected_importer_ids,
        prune_explicit_peers,
    );
    Ok(built)
}
fn prune_uninstalled_explicit_peers(
    lockfile: &mut Lockfile,
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    selected_importer_ids: Option<&std::collections::HashSet<String>>,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    // Explicitly selected peers must reach resolution so update can settle
    // their manifest ranges. With automatic installation disabled they do not
    // belong in the saved importer, so remove the temporary direct entries and
    // any package snapshots that only those entries reached.
    for (importer_id, manifest) in importer_manifests {
        if selected_importer_ids.is_some_and(|ids| !ids.contains(importer_id)) {
            continue;
        }
        if let Some(importer) = lockfile.importers.get_mut(importer_id) {
            pnpm_lockfile::prune_undeclared_importer_deps(importer, None, manifest, false);
        }
    }
    crate::fast_update_lockfile::prune_unreachable_packages(lockfile);
}
impl<'a> FreshLockfileInputs<'a> {
    fn importer_entries(&self) -> BTreeMap<String, ImporterLockfileInput<'a>> {
        let mut importers = BTreeMap::new();
        for (id, manifest) in self.importer_manifests {
            let direct = self.resolution.direct_by_importer
                .get(id)
                .cloned()
                .unwrap_or_default();
            let manifest = self.lockfile_specifier_manifests
                .and_then(|manifests| manifests.get(id))
                .unwrap_or(*manifest);
            importers.insert(
                id.clone(),
                ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct },
            );
        }
        importers
    }
}
pub(super) fn build_fresh_lockfile(
    inputs: FreshLockfileInputs<'_>,
) -> Result<Lockfile, DependenciesGraphToLockfileError> {
    let importers = inputs.importer_entries();
    let registries_by_prefix = registries_by_prefix(inputs.config);
    let config = inputs.config;
    dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: inputs.resolution.graph,
        catalogs: inputs.catalogs,
        time: merge_recorded_time(inputs.prior.lockfile, inputs.resolution.time),
        settings: pnpm_lockfile::LockfileSettings {
            auto_install_peers: config.auto_install_peers,
            dedupe_peers: config.dedupe_peers.then_some(true),
            exclude_links_from_lockfile: config.exclude_links_from_lockfile,
            inject_workspace_packages: config.inject_workspace_packages,
            peers_suffix_max_length: (config.peers_suffix_max_length
                != pnpm_config::default_peers_suffix_max_length())
            .then_some(config.peers_suffix_max_length),
        },
        metadata_sources: crate::PackageMetadataSources {
            registry_options_by_url: &config.registry_options_by_url,
            registry: &config.registry,
            registries_by_prefix: &registries_by_prefix,
            lockfile_include_tarball_url: config.lockfile_include_tarball_url,
            previous_packages: inputs.prior.lockfile.and_then(|lockfile| {
                lockfile.packages.as_ref()
            }),
        },
        manifest_settings: inputs.manifest_settings.build(
            config,
            inputs.resolution.overrides,
            inputs.resolution.include_peer_dependencies,
        ),
        reuse: crate::LockfileImporterReuse {
            previous_importers: inputs.prior.importers,
            scope: inputs.prior.scope,
            scopes_by_importer: inputs.prior.scopes_by_importer,
        },
    })
}
/// Same merge the resolver chain performs; the config was already
/// validated at resolver construction, so skip re-validation here.
pub(super) fn registries_by_prefix(config: &Config) -> HashMap<String, String> {
    pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX
        .iter()
        .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
        .chain(
            config.registries_by_prefix
                .iter()
                .map(|(name, url)| (name.clone(), url.clone())),
        )
        .collect()
}
/// The `time:` section the rewritten lockfile carries: what the prior
/// lockfile recorded, with this run's freshly resolved publish dates
/// layered over it. Keeping the prior entries is what preserves a
/// recorded date for a dependency whose packument does not carry one;
/// saving prunes whatever is no longer a direct dependency.
pub(super) fn merge_recorded_time(
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
    let extensions = config.package_extensions
        .as_ref()
        .filter(|extensions| !extensions.is_empty())?;
    let value = serde_json::to_value(extensions).ok()?;
    pnpm_graph_hasher::hash_object_nullable_with_prefix(&value)
}

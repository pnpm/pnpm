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
    let pnpmfile_checksum = pnpmfile_checksum(resolved.after_all_resolved_hook.as_ref()).await;
    if install.lockfile_only {
        await_lockfile_gate(lockfile_verification_gate).await?;
    }
    let phase_start = std::time::Instant::now();
    let built_lockfile = build_resolved_lockfile(
        install,
        resolved,
        views,
        resolved_time,
        pnpmfile_checksum.as_deref(),
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
) -> Result<Lockfile, InstallWithFreshLockfileError> {
    build_lockfile(FreshLockfileBuildOptions {
        inputs: FreshLockfileInputs {
            config: install.config,
            importer_manifests: views.importer_manifests,
            lockfile_specifier_manifests: views.lockfile_specifier_manifests,
            graph: &resolved.merged_graph,
            direct_by_importer: &resolved.direct_by_importer,
            resolved_overrides: resolved.overrides.clone(),
            catalogs: views.catalogs,
            pnpmfile_checksum,
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
    })
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
    parsed.iter().map(|entry| (entry.selector.clone(), entry.new_bare_specifier.clone())).collect()
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
                && lockfile.iter().all(|(key, value)| {
                    config.get(key).is_some_and(|config_value| config_value == value)
                })
        }
        _ => false,
    }
}
pub(super) fn ignored_optional_dependencies_match(
    left: Option<&[String]>,
    right: Option<&[String]>,
) -> bool {
    let left: HashSet<_> = left.unwrap_or_default().iter().collect();
    let right: HashSet<_> = right.unwrap_or_default().iter().collect();
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
/// See [`FreshInputs::manifest_spec_bumps`].
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
        let overridden =
            self.versions_overrider.filter(|overrider| !overrider.is_empty()).map(|overrider| {
                crate::manifest_spec_bumps::OverriddenDeclarations { overrider, importer_manifests }
            });
        crate::manifest_spec_bumps::apply_manifest_spec_bumps(built, bumps, overridden.as_ref());
    }
}
pub(super) fn build_lockfile(
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
pub(super) fn build_fresh_lockfile(
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
pub(super) fn registries_by_prefix(config: &Config) -> HashMap<String, String> {
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
    let extensions =
        config.package_extensions.as_ref().filter(|extensions| !extensions.is_empty())?;
    let value = serde_json::to_value(extensions).ok()?;
    pnpm_graph_hasher::hash_object_nullable_with_prefix(&value)
}

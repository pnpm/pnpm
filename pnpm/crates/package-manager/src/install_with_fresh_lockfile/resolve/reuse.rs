use super::super::{ImporterUpdateSeedPolicy, UpdateSeedPolicy};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{ManifestHook, UpdateTargets};
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions};
use std::{
    collections::{BTreeMap, HashMap},
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
pub(in super::super) fn preferred_versions_seeds(
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
        by_importer = by_importer_seeds(policies, snapshots, &manifests, overrides);
    }

    (Arc::new(workspace_seed), by_importer)
}
/// One seed per importer, with the two seed shapes cached: a `DropAll`
/// importer always seeds the same way, and importers naming the same update
/// targets share one `DropOnly` seed.
pub(super) fn by_importer_seeds(
    policies: &BTreeMap<String, ImporterUpdateSeedPolicy>,
    snapshots: Option<&HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>>,
    manifests: &[&PackageManifest],
    overrides: Option<&PreferredVersions>,
) -> BTreeMap<String, Arc<PreferredVersions>> {
    let mut by_importer = BTreeMap::new();
    let mut drop_all_seed = None;
    let mut drop_only_seeds = HashMap::new();
    for (importer_id, policy) in policies {
        let seed = match policy {
            ImporterUpdateSeedPolicy::DropAll => Arc::clone(drop_all_seed.get_or_insert_with(|| {
                let mut seed =
                    pnpm_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests(
                        None, manifests,
                    );
                merge_preferred_versions(&mut seed, overrides);
                Arc::new(seed)
            })),
            ImporterUpdateSeedPolicy::DropOnly(targets) => {
                drop_only_seed(&mut drop_only_seeds, targets, snapshots, manifests, overrides)
            }
        };
        by_importer.insert(importer_id.clone(), seed);
    }
    by_importer
}
pub(super) fn drop_only_seed(
    cache: &mut HashMap<UpdateTargets, Arc<PreferredVersions>>,
    targets: &UpdateTargets,
    snapshots: Option<&HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>>,
    manifests: &[&PackageManifest],
    overrides: Option<&PreferredVersions>,
) -> Arc<PreferredVersions> {
    if let Some(seed) = cache.get(targets) {
        return Arc::clone(seed);
    }
    let mut seed =
        pnpm_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests_excluding(
            snapshots,
            manifests,
            &withheld_pin(targets),
        );
    merge_preferred_versions(&mut seed, overrides);
    let seed = Arc::new(seed);
    cache.insert(targets.clone(), Arc::clone(&seed));
    seed
}
/// Layer caller-supplied preferences onto a seed, per package name. A
/// selector present in both wins from `overrides`, which is how a version
/// named on the command line outranks the pin the lockfile seeded for it.
pub(super) fn merge_preferred_versions(
    seed: &mut PreferredVersions,
    overrides: Option<&PreferredVersions>,
) {
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
pub(super) fn withheld_pin(
    targets: &UpdateTargets,
) -> impl Fn(&pnpm_lockfile::PackageKey) -> bool + '_ {
    |key| targets.covers(key.name.to_string().as_str(), key.suffix.version_semver())
}
pub(in super::super) struct ReuseSeedInputs<'a> {
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
impl ReuseSeedInputs<'_> {
    fn package_settings_match(&self, lockfile: &Lockfile) -> bool {
        lockfile.package_extensions_checksum.as_deref() == self.package_extensions_checksum
            && super::super::ignored_optional_dependencies_match(
                lockfile.ignored_optional_dependencies.as_deref(),
                self.config.ignored_optional_dependencies.as_deref(),
            )
    }

    fn rewrite_context<'c>(
        &'c self,
        lockfile: &'c Lockfile,
        manifest_hook: Option<&'c ManifestHook>,
    ) -> crate::fast_update_overrides::RewriteContext<'c> {
        crate::fast_update_overrides::RewriteContext {
            lockfile,
            resolver: self.npm_resolver,
            resolve_options: self.resolve_options,
            manifest_hook,
            registries: self.registries,
            registry_options_by_url: &self.config.registry_options_by_url,
            lockfile_include_tarball_url: self.config.lockfile_include_tarball_url,
        }
    }
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
pub(in super::super) async fn lockfile_reuse_seed(
    inputs: ReuseSeedInputs<'_>,
) -> Option<Arc<Lockfile>> {
    use crate::fast_update_catalogs::{FastCatalogUpdate, try_fast_update_catalogs};

    let overrides_use_catalogs = overrides_use_catalogs(inputs.config);
    let (catalogs_match, fast_catalog_seed) =
        match inputs.wanted_lockfile.map_or(FastCatalogUpdate::Unchanged, |lockfile| {
            try_fast_update_catalogs(lockfile, inputs.catalogs, overrides_use_catalogs)
        }) {
            FastCatalogUpdate::Unchanged => (true, None),
            FastCatalogUpdate::Updated(lockfile) => (false, Some(*lockfile)),
            FastCatalogUpdate::Unsupported => (false, None),
        };

    let lockfile =
        inputs.wanted_lockfile.filter(|lockfile| inputs.package_settings_match(lockfile))?;
    let override_settings_match =
        super::super::overrides_match(lockfile.overrides.as_ref(), inputs.resolved_overrides);

    let rewrite_manifest_hook = super::super::compose_manifest_hooks(
        inputs.manifest_hook.clone(),
        inputs.overrides_hook.clone(),
    );
    // A catalog move can change the effective value of an override whose
    // configured value is a `catalog:` reference — an effect no catalog
    // rewrite can express — so catalog drift under such an override goes to
    // the resolver. The override rewrite itself is safe under one: it runs
    // only once the catalogs are settled, and override values are compared
    // catalog-resolved, so a settled `catalog:` override shows no drift and
    // only the genuinely changed entries are rewritten.
    let can_rewrite_catalogs = inputs.fast_override_eligible && !overrides_use_catalogs;

    let catalog_rewrite = match rewritten_catalogs(
        CatalogRewriteInputs { catalogs_match, fast_catalog_seed, can_rewrite_catalogs },
        inputs.rewrite_context(lockfile, rewrite_manifest_hook.as_ref()),
        inputs.catalogs,
    )
    .await
    {
        CatalogRewrite::Unsupported => return None,
        CatalogRewrite::Unchanged => None,
        CatalogRewrite::Rewritten(seed) => Some(*seed),
    };

    reuse_or_rewrite_overrides(
        &inputs,
        lockfile,
        catalog_rewrite,
        override_settings_match,
        rewrite_manifest_hook.as_ref(),
    )
    .await
}
pub(super) async fn reuse_or_rewrite_overrides(
    inputs: &ReuseSeedInputs<'_>,
    lockfile: &Lockfile,
    catalog_rewrite: Option<Lockfile>,
    override_settings_match: bool,
    rewrite_manifest_hook: Option<&pnpm_resolving_deps_resolver::ManifestHook>,
) -> Option<Arc<Lockfile>> {
    use crate::fast_update_overrides::{FastOverrideOptions, try_fast_update_overrides};
    if override_settings_match {
        return Some(match catalog_rewrite {
            Some(rewritten) => Arc::new(rewritten),
            // `lockfile` is `wanted_lockfile` narrowed by the filter
            // above, so the loader's handle to it reuses the parsed
            // document verbatim.
            None => {
                inputs.wanted_lockfile_shared.map_or_else(|| Arc::new(lockfile.clone()), Arc::clone)
            }
        });
    }
    if !inputs.fast_override_eligible {
        return None;
    }
    let seed = try_fast_update_overrides(FastOverrideOptions {
        context: inputs
            .rewrite_context(catalog_rewrite.as_ref().unwrap_or(lockfile), rewrite_manifest_hook),
        parsed_overrides: inputs.parsed_overrides?,
        resolved_overrides: inputs.resolved_overrides?,
    })
    .await?;
    Some(Arc::new(seed))
}
pub(super) fn overrides_use_catalogs(config: &Config) -> bool {
    config
        .overrides
        .as_ref()
        .is_some_and(|overrides| overrides.values().any(|value| value.starts_with("catalog:")))
}
/// What the workspace's catalogs did to the lockfile the reuse seed starts
/// from.
pub(super) enum CatalogRewrite {
    /// The catalogs are unchanged, so the lockfile stands.
    Unchanged,
    Rewritten(Box<Lockfile>),
    /// The move needs a resolution.
    Unsupported,
}
/// What the catalog rewrite already knows before it consults the resolver.
pub(super) struct CatalogRewriteInputs {
    catalogs_match: bool,
    /// The seed the range-only retarget produced, when it could.
    fast_catalog_seed: Option<Lockfile>,
    can_rewrite_catalogs: bool,
}
pub(super) async fn rewritten_catalogs(
    inputs: CatalogRewriteInputs,
    context: crate::fast_update_overrides::RewriteContext<'_>,
    catalogs: &Catalogs,
) -> CatalogRewrite {
    if inputs.catalogs_match {
        return CatalogRewrite::Unchanged;
    }
    if let Some(seed) = inputs.fast_catalog_seed {
        return CatalogRewrite::Rewritten(Box::new(seed));
    }
    if !inputs.can_rewrite_catalogs {
        return CatalogRewrite::Unsupported;
    }
    // A catalog entry that now names a version the locked one cannot
    // satisfy left `catalogs_match` false with no seed above. Replacing
    // the package is the same rewrite an exact override performs.
    match crate::fast_update_catalog_versions::try_fast_update_catalog_versions(&context, catalogs)
        .await
    {
        Some(seed) => CatalogRewrite::Rewritten(Box::new(seed)),
        None => CatalogRewrite::Unsupported,
    }
}

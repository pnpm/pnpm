use super::resolve;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_resolving_deps_resolver::{ResolveWorkspaceResult, UpdateTargets};
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, PreferredVersions, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

/// Reopen duplicate names while the normal reuse gate handles changed requirements.
/// Newly discovered versions extend these targets in the next resolution round.
pub(super) fn targets(
    config: &Config,
    lockfile: Option<&Lockfile>,
    can_reuse: bool,
) -> UpdateTargets {
    let Some(lockfile) = lockfile.filter(|_| config.auto_dedupe) else {
        return UpdateTargets::default();
    };
    let mut versions = HashMap::<String, HashSet<String>>::new();
    for key in lockfile.packages.iter().flat_map(|packages| packages.keys()) {
        versions
            .entry(key.name.to_string())
            .or_default()
            .insert(key.suffix.to_string());
    }
    versions
        .into_iter()
        .filter(|(_, versions)| !can_reuse || versions.len() > 1)
        .map(|(name, _)| (name, None))
        .collect()
}

pub(super) fn prefer_new_candidates(
    result: &ResolveWorkspaceResult,
    preferred: &mut Arc<PreferredVersions>,
    by_importer: &mut BTreeMap<String, Arc<PreferredVersions>>,
    targets: &mut UpdateTargets,
) -> bool {
    let mut versions = HashMap::<String, HashSet<String>>::new();
    for node in result.peers.graph.values() {
        let Some(identity) = node.resolve_result.package.name_ver.as_ref() else { continue };
        versions
            .entry(identity.name.to_string())
            .or_default()
            .insert(identity.suffix.to_string());
    }
    let mut changed = false;
    for (name, versions) in versions
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
    {
        let mut name_changed = false;
        for seed in std::iter::once(&mut *preferred).chain(by_importer.values_mut()) {
            for version in &versions {
                name_changed |= prefer_candidate(seed, &name, version);
            }
        }
        if name_changed {
            targets.insert(name, None);
            changed = true;
        }
    }
    changed
}

fn prefer_candidate(preferred: &mut Arc<PreferredVersions>, name: &str, version: &str) -> bool {
    let existing = preferred
        .get(name)
        .and_then(|selectors| selectors.get(version));
    if matches!(existing, Some(VersionSelectorEntry::Weighted(entry)) if entry.weight >= EXISTING_VERSION_SELECTOR_WEIGHT)
    {
        return false;
    }
    Arc::make_mut(preferred)
        .entry(name.to_string())
        .or_default()
        .insert(
            version.to_string(),
            VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                selector_type: VersionSelectorType::Version,
                weight: EXISTING_VERSION_SELECTOR_WEIGHT,
            }),
        );
    true
}

impl<Reporter: pnpm_reporter::Reporter + 'static> super::ResolutionContext<'_, Reporter> {
    pub(super) async fn resolve_rounds(
        &self,
        importer_manifests: &BTreeMap<String, &pnpm_package_manifest::PackageManifest>,
        lockfile_reuse_seed: Option<Arc<Lockfile>>,
        mut preferred_versions_seed: Arc<PreferredVersions>,
        mut preferred_versions_seeds_by_importer: BTreeMap<String, Arc<PreferredVersions>>,
    ) -> Result<ResolveWorkspaceResult, super::InstallWithFreshLockfileError> {
        let shared_resolve_options = self.shared_options();
        let mut dedupe = targets(
            self.install.drivers.config,
            self.wanted_lockfile(),
            lockfile_reuse_seed.is_some(),
        );
        loop {
            let walk = self.workspace_walk(lockfile_reuse_seed.as_ref(), dedupe.clone());
            let result = resolve::run_resolve_pass::<Reporter>(resolve::ResolvePassInputs {
                resolver: &*self.setup.chain.resolver,
                importer_manifests,
                dependency_groups: self.install.resolved_groups(),
                walk,
                per_importer: self.importer_inputs(
                    &shared_resolve_options,
                    &preferred_versions_seed,
                    &preferred_versions_seeds_by_importer,
                ),
            })
            .await?;
            if !self.install.drivers.config.auto_dedupe
                || !prefer_new_candidates(
                    &result,
                    &mut preferred_versions_seed,
                    &mut preferred_versions_seeds_by_importer,
                    &mut dedupe,
                )
            {
                return Ok(result);
            }
        }
    }
}

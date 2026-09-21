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

fn extend_preference_seeds(
    versions: &PreferredVersions,
    preferred: &mut Arc<PreferredVersions>,
    by_importer: &mut BTreeMap<String, Arc<PreferredVersions>>,
    targets: &mut UpdateTargets,
) -> bool {
    if versions.is_empty() {
        return false;
    }
    let mut groups = HashMap::<_, Vec<&mut Arc<PreferredVersions>>>::new();
    for seed in std::iter::once(preferred).chain(by_importer.values_mut()) {
        groups
            .entry(Arc::as_ptr(seed))
            .or_default()
            .push(seed);
    }
    let mut changed = false;
    for seeds in groups.into_values() {
        let mut updated = Arc::clone(seeds[0]);
        changed |= prefer_versions(&mut updated, versions, targets);
        for seed in seeds {
            *seed = Arc::clone(&updated);
        }
    }
    changed
}

fn prefer_versions(
    preferred: &mut Arc<PreferredVersions>,
    versions: &PreferredVersions,
    targets: &mut UpdateTargets,
) -> bool {
    let mut changed = false;
    for (name, versions) in versions {
        for version in versions.keys() {
            if prefer_candidate(preferred, name, version) {
                targets.insert(name.clone(), None);
                changed = true;
            }
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
            let result = resolve::run_dependency_pass::<Reporter>(resolve::ResolvePassInputs {
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
                || !extend_preference_seeds(
                    &result.duplicate_versions(),
                    &mut preferred_versions_seed,
                    &mut preferred_versions_seeds_by_importer,
                    &mut dedupe,
                )
            {
                return result
                    .resolve_peers(&*self.setup.chain.resolver)
                    .await
                    .map_err(resolve::resolve_error);
            }
        }
    }
}

#[cfg(test)]
mod tests;

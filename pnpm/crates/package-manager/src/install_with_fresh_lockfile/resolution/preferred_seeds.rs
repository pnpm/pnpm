use super::{ResolutionContext, resolve};
use pnpm_lockfile_preferred_versions::{
    DirectSpecs, SeededNames, get_preferred_versions_from_lockfile_and_manifests,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::UpdateTargets;
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

impl<Reporter: self::Reporter + 'static> ResolutionContext<'_, Reporter> {
    pub(super) fn preferred_versions_seeds(
        &self,
        importer_manifests: &BTreeMap<String, &PackageManifest>,
        stale_override_targets: &UpdateTargets,
    ) -> (Arc<PreferredVersions>, BTreeMap<String, Arc<PreferredVersions>>) {
        let manifests: Vec<&PackageManifest> = importer_manifests
            .values()
            .copied()
            .collect();
        let peer_names: HashSet<String>;
        let names = if self.dedupes_across_projects() {
            SeededNames::All
        } else {
            peer_names = workspace_peer_names(&manifests);
            SeededNames::Only(&peer_names)
        };
        resolve::preferred_versions_seeds(
            &self.owned.resolution.update_seed_policy,
            self.wanted_lockfile(),
            DirectSpecs { manifests: &manifests, catalogs: &self.owned.projects.catalogs, names },
            self.owned.resolution.preferred_versions_override.as_ref(),
            stale_override_targets,
        )
    }

    /// Whether every project's manifest specifiers steer the version picks of
    /// the whole workspace. Without it, a project resolves its dependencies
    /// from its own ranges, as it would with its own lockfile. Only the names
    /// some project declares as a peer still follow the other projects.
    fn dedupes_across_projects(&self) -> bool {
        self.install.drivers.config.auto_dedupe
            || matches!(
                self.owned.resolution.update_seed_policy,
                super::super::UpdateSeedPolicy::KeepAllResolveAll,
            )
    }

    /// The manifest specifiers of every project, for the peer hoister, when
    /// [`Self::dedupes_across_projects`] keeps them out of the seed.
    pub(super) fn manifest_specifiers(
        &self,
        importer_manifests: &BTreeMap<String, &PackageManifest>,
    ) -> Option<Arc<PreferredVersions>> {
        if self.dedupes_across_projects() {
            return None;
        }
        let manifests: Vec<&PackageManifest> = importer_manifests
            .values()
            .copied()
            .collect();
        Some(Arc::new(get_preferred_versions_from_lockfile_and_manifests(
            None,
            DirectSpecs {
                manifests: &manifests,
                catalogs: &self.owned.projects.catalogs,
                names: SeededNames::All,
            },
        )))
    }
}

fn workspace_peer_names(manifests: &[&PackageManifest]) -> HashSet<String> {
    manifests
        .iter()
        .flat_map(|manifest| manifest.dependencies([DependencyGroup::Peer]))
        .map(|(name, _)| name.to_string())
        .collect()
}

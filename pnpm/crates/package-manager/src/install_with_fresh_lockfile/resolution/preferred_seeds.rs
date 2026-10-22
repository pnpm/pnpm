use super::{ResolutionContext, resolve};
use crate::install_with_fresh_lockfile::resolution_inputs::OwnExactVersions;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile_preferred_versions::{
    DirectSpecs, SeededNames, get_preferred_versions_from_lockfile_and_manifests,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::UpdateTargets;
use pnpm_resolving_resolver_base::{
    PreferredVersions, PreferredVersionsOverlay, VersionSelectorType,
};
use rayon::prelude::*;
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

    /// Each project's own exact specifiers, when
    /// [`Self::dedupes_across_projects`] keeps them out of the seed.
    pub(super) fn own_exact_versions(
        &self,
        importer_manifests: &BTreeMap<String, &PackageManifest>,
    ) -> Option<OwnExactVersions> {
        if self.dedupes_across_projects() {
            return None;
        }
        let catalogs = &self.owned.projects.catalogs;
        importer_manifests
            .par_iter()
            .filter_map(|(id, manifest)| {
                Some((id.clone(), exact_versions_layer(manifest, catalogs)?))
            })
            .collect::<OwnExactVersions>()
            .pipe(Some)
    }
}

fn workspace_peer_names(manifests: &[&PackageManifest]) -> HashSet<String> {
    manifests
        .iter()
        .flat_map(|manifest| manifest.dependencies([DependencyGroup::Peer]))
        .map(|(name, _)| name.to_string())
        .collect()
}

fn exact_versions_layer(
    manifest: &PackageManifest,
    catalogs: &Catalogs,
) -> Option<Arc<PreferredVersionsOverlay>> {
    let direct = DirectSpecs { manifests: &[manifest], catalogs, names: SeededNames::All };
    let entries = get_preferred_versions_from_lockfile_and_manifests(None, direct)
        .into_iter()
        .filter_map(|(name, selectors)| {
            let versions: Vec<String> = selectors
                .into_iter()
                .filter(|(_, entry)| entry.selector_type() == VersionSelectorType::Version)
                .map(|(version, _)| version)
                .collect();
            (!versions.is_empty()).then_some((name, versions))
        })
        .collect();
    PreferredVersionsOverlay::manifest_specifiers(entries)
}

use super::{ResolutionContext, resolve};
use crate::install_with_fresh_lockfile::resolution_inputs::OwnManifestSpecifiers;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile_preferred_versions::{
    DirectSpecs, get_preferred_versions_from_lockfile_and_manifests,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::UpdateTargets;
use pnpm_resolving_resolver_base::{PreferredVersions, PreferredVersionsOverlay, VersionSelectors};
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
        let manifests: Vec<&PackageManifest> = if self.dedupes_across_projects() {
            importer_manifests
                .values()
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        resolve::preferred_versions_seeds(
            &self.owned.resolution.update_seed_policy,
            self.wanted_lockfile(),
            DirectSpecs { manifests: &manifests, catalogs: &self.owned.projects.catalogs },
            self.owned.resolution.preferred_versions_override.as_ref(),
            stale_override_targets,
        )
    }

    /// Whether every project's manifest specifiers steer the version picks of
    /// the whole workspace. Without it, a project resolves its dependencies
    /// from its own ranges, as it would with its own lockfile, and only its
    /// peer-only `peerDependencies` follow the other projects.
    fn dedupes_across_projects(&self) -> bool {
        self.install.drivers.config.auto_dedupe
            || matches!(
                self.owned.resolution.update_seed_policy,
                super::super::UpdateSeedPolicy::KeepAllResolveAll,
            )
    }

    /// Each project's own manifest specifiers, when
    /// [`Self::dedupes_across_projects`] keeps them out of the seed.
    pub(super) fn own_manifest_specifiers(
        &self,
        importer_manifests: &BTreeMap<String, &PackageManifest>,
    ) -> Option<OwnManifestSpecifiers> {
        if self.dedupes_across_projects() {
            return None;
        }
        own_manifest_specifiers(importer_manifests, &self.owned.projects.catalogs).pipe(Some)
    }
}

fn own_manifest_specifiers(
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    catalogs: &Catalogs,
) -> OwnManifestSpecifiers {
    let mut by_importer: BTreeMap<String, PreferredVersions> = importer_manifests
        .par_iter()
        .map(|(id, manifest)| {
            let direct = DirectSpecs { manifests: &[manifest], catalogs };
            (id.clone(), get_preferred_versions_from_lockfile_and_manifests(None, direct))
        })
        .collect();
    let peer_additions: Vec<(String, String, VersionSelectors)> = importer_manifests
        .iter()
        .flat_map(|(id, manifest)| {
            peer_only_names(manifest)
                .into_iter()
                .map(|peer_name| {
                    let selectors: VersionSelectors = by_importer
                        .values()
                        .filter_map(|preferred| preferred.get(peer_name))
                        .flatten()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    (id.clone(), peer_name.to_string(), selectors)
                })
        })
        .filter(|(_, _, selectors)| !selectors.is_empty())
        .collect();
    for (id, peer_name, selectors) in peer_additions {
        by_importer
            .entry(id)
            .or_default()
            .entry(peer_name)
            .or_default()
            .extend(selectors);
    }
    by_importer
        .into_iter()
        .filter_map(|(id, preferred)| {
            Some((id, PreferredVersionsOverlay::manifest_specifiers(preferred)?))
        })
        .collect()
}

fn peer_only_names(manifest: &PackageManifest) -> Vec<&str> {
    if manifest
        .dependencies([DependencyGroup::Peer])
        .next()
        .is_none()
    {
        return Vec::new();
    }
    let direct: HashSet<&str> = manifest
        .dependencies([DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional])
        .map(|(name, _)| name)
        .collect();
    manifest
        .dependencies([DependencyGroup::Peer])
        .map(|(name, _)| name)
        .filter(|name| !direct.contains(name))
        .collect()
}

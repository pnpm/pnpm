//! The peers of a local directory dependency, as the resolver reads them.

use super::{LocalDepContext, optional_peer_names};
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{HashMap, HashSet};

/// How the resolver treats the names a package declares both as one of its
/// own dependencies and as a peer (see the resolver's
/// `peer_shadowed_dependencies` and `extract_peer_dependencies`).
pub(super) struct PeerShadowing<'m> {
    /// The `dependencies` entries the peer edge supplies instead, so the
    /// snapshot records the peer's resolution for them. Under
    /// `autoInstallPeers` every non-optional peer shadows its dependency;
    /// otherwise only one the parent scope provides does, which the lockfile
    /// records by keeping the name among the package's peers.
    pub(super) shadowed: HashSet<&'m str>,
    /// The names the resolver resolves as the package's own dependencies,
    /// which it therefore records as no peer: the unshadowed
    /// `dependencies`, every `optionalDependencies` entry and the package's
    /// own name.
    own: HashSet<&'m str>,
}

impl<'m> PeerShadowing<'m> {
    pub(super) fn of(
        manifest: &'m PackageManifest,
        pkg_meta: &pnpm_lockfile::PackageMetadata,
        auto_install_peers: bool,
    ) -> Self {
        let peers: HashSet<&str> = manifest
            .dependencies([DependencyGroup::Peer])
            .map(|(name, _)| name)
            .collect();
        let optional_peers: HashSet<&str> = optional_peer_names(manifest).collect();
        let recorded = pkg_meta.peer_dependencies.as_ref();
        let (shadowed, mut own): (HashSet<&str>, HashSet<&str>) = manifest
            .dependencies([DependencyGroup::Prod])
            .map(|(name, _)| name)
            .partition(|name| {
                peers.contains(name)
                    && ((auto_install_peers && !optional_peers.contains(name))
                        || recorded.is_some_and(|recorded| recorded.contains_key(*name)))
            });
        own.extend(
            manifest
                .dependencies([DependencyGroup::Optional])
                .map(|(name, _)| name),
        );
        own.extend(
            manifest
                .value()
                .get("name")
                .and_then(serde_json::Value::as_str),
        );
        PeerShadowing { shadowed, own }
    }

    /// Compares only the declared peer ranges with the recorded ones. The
    /// resolved peers in the snapshot are whatever the parent provides, such as
    /// a `link:` to a workspace project or a version outside the range (an unmet
    /// peer only warns), so they say nothing about whether the lockfile is stale.
    pub(super) fn check_local_peer_deps_freshness(
        &self,
        dep: &LocalDepContext<'_>,
        local_manifest: &PackageManifest,
        pkg_meta: &pnpm_lockfile::PackageMetadata,
    ) -> Result<(), FreshnessCheckError> {
        let mut manifest_peers: HashMap<&str, &str> = local_manifest
            .dependencies([DependencyGroup::Peer])
            .filter(|(name, _)| !self.own.contains(name))
            .collect();
        for name in self.optional_peer_names(local_manifest) {
            manifest_peers.entry(name).or_insert("*");
        }
        check_recorded_peer_specs_match(dep, &manifest_peers, pkg_meta)?;
        self.check_peer_dependencies_meta_freshness(dep, local_manifest, pkg_meta)
    }

    fn optional_peer_names<'a>(
        &'a self,
        manifest: &'a PackageManifest,
    ) -> impl Iterator<Item = &'a str> {
        optional_peer_names(manifest).filter(|name| !self.own.contains(name))
    }

    fn check_peer_dependencies_meta_freshness(
        &self,
        dep: &LocalDepContext<'_>,
        local_manifest: &PackageManifest,
        pkg_meta: &pnpm_lockfile::PackageMetadata,
    ) -> Result<(), FreshnessCheckError> {
        let manifest_meta = local_manifest
            .value()
            .get("peerDependenciesMeta")
            .and_then(serde_json::Value::as_object);
        let recorded_meta = pkg_meta.peer_dependencies_meta.as_ref();
        let manifest_optional_count = self.optional_peer_names(local_manifest).count();
        let recorded_optional_count = recorded_meta.map_or(0, |meta| {
            meta.values()
                .filter(|m| m.optional)
                .count()
        });
        if manifest_optional_count != recorded_optional_count {
            return Err(dep.outdated());
        }
        if let Some(recorded) = recorded_meta {
            for (name, meta) in recorded {
                let manifest_optional = manifest_meta
                    .and_then(|m| m.get(name))
                    .and_then(|entry| entry.get("optional"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if meta.optional != manifest_optional {
                    return Err(dep.outdated());
                }
            }
        }
        Ok(())
    }
}

pub(super) fn check_recorded_peer_specs_match(
    dep: &LocalDepContext<'_>,
    manifest_peers: &HashMap<&str, &str>,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let recorded_count = pkg_meta.peer_dependencies.as_ref().map_or(0, HashMap::len);
    if manifest_peers.len() != recorded_count {
        return Err(dep.outdated());
    }
    for (name, spec) in manifest_peers {
        let recorded_spec = pkg_meta.peer_dependencies
            .as_ref()
            .and_then(|p| p.get(*name));
        if recorded_spec.map(String::as_str) == Some(spec) {
            continue;
        }
        if dep.catalogs.is_empty() {
            return Err(dep.outdated());
        }
        let wanted =
            WantedDependency { alias: (*name).to_string(), bare_specifier: (*spec).to_string() };
        match resolve_from_catalog(dep.catalogs, &wanted, CatalogAnchor::AsWritten) {
            CatalogResolutionResult::Found(found)
                if recorded_spec == Some(&found.resolution.specifier) => {}
            CatalogResolutionResult::Misconfiguration(misconfiguration) => {
                return Err(FreshnessCheckError::InvalidCatalog(misconfiguration.error));
            }
            _ => return Err(dep.outdated()),
        }
    }
    Ok(())
}

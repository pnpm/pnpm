//! The peers of a local directory dependency, as the resolver reads them.

use super::LocalDepContext;
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    PeerSet, declared_peer_dependencies, own_dependency_names, peer_shadowed_dependency_names,
};
use std::collections::{HashMap, HashSet};

/// How the resolver treats the names a package declares both as one of its
/// own dependencies and as a peer, decided by the resolver's own rule in
/// the scope the package resolved in.
pub(super) struct PeerShadowing<'m> {
    /// The `dependencies` entries the peer edge supplies instead, so the
    /// snapshot records the peer's resolution for them. See
    /// [`peer_shadowed_dependency_names`].
    pub(super) shadowed: HashSet<&'m str>,
    /// See [`own_dependency_names`].
    own: HashSet<&'m str>,
}

impl<'m> PeerShadowing<'m> {
    pub(super) fn of(
        manifest: &'m PackageManifest,
        in_scope: impl Fn(&str) -> bool,
        auto_install_peers: bool,
    ) -> Self {
        let shadowed: HashSet<&str> =
            peer_shadowed_dependency_names(Some(manifest.value()), in_scope, auto_install_peers)
                .collect();
        let own = own_dependency_names(manifest.value(), |name| shadowed.contains(name)).collect();
        PeerShadowing { shadowed, own }
    }

    /// Compares only the declared peer ranges with the recorded ones. The
    /// resolved peers in the snapshot are whatever the parent provides, such as
    /// a `link:` to a workspace project or a version outside the range (an unmet
    /// peer only warns), so they say nothing about whether the lockfile is stale.
    ///
    /// The lockfile records every [`declared_peer_dependencies`] entry, and one
    /// written before it did records only the peers the package takes in its
    /// scope, so both are fresh.
    pub(super) fn check_local_peer_deps_freshness(
        &self,
        dep: &LocalDepContext<'_>,
        local_manifest: &PackageManifest,
        pkg_meta: &pnpm_lockfile::PackageMetadata,
    ) -> Result<(), FreshnessCheckError> {
        let peer_set = PeerSet::new(declared_peer_dependencies(local_manifest.value()), |name| {
            self.own.contains(name)
        });
        let declared = peer_set.declared();
        let taken: HashSet<&str> = peer_set
            .taken()
            .keys()
            .map(String::as_str)
            .collect();
        let declared_specs: HashMap<&str, &str> = declared
            .iter()
            .map(|(name, peer)| (name.as_str(), peer.version.as_str()))
            .collect();
        check_recorded_peer_specs_match(dep, &declared_specs, &taken, pkg_meta)?;
        let recorded_optional: HashSet<&str> = pkg_meta.peer_dependencies_meta
            .iter()
            .flatten()
            .filter(|(_, meta)| meta.optional)
            .map(|(name, _)| name.as_str())
            .collect();
        let optional_matches = recorded_optional
            .iter()
            .all(|name| declared.get(*name).is_some_and(|peer| peer.optional))
            && declared
                .iter()
                .filter(|(name, peer)| peer.optional && taken.contains(name.as_str()))
                .all(|(name, _)| recorded_optional.contains(name.as_str()));
        if !optional_matches {
            return Err(dep.outdated());
        }
        Ok(())
    }
}

/// Every recorded peer is `declared` with the recorded range, and every
/// `taken` one is recorded.
pub(super) fn check_recorded_peer_specs_match(
    dep: &LocalDepContext<'_>,
    declared: &HashMap<&str, &str>,
    taken: &HashSet<&str>,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let recorded = pkg_meta.peer_dependencies.as_ref();
    let recorded_names = || recorded.into_iter().flat_map(HashMap::keys);
    if recorded_names().any(|name| !declared.contains_key(name.as_str()))
        || taken
            .iter()
            .any(|name| recorded.is_none_or(|recorded| !recorded.contains_key(*name)))
    {
        return Err(dep.outdated());
    }
    for name in recorded_names() {
        let spec = declared[name.as_str()];
        let recorded_spec = recorded.and_then(|p| p.get(name));
        if recorded_spec.map(String::as_str) == Some(spec) {
            continue;
        }
        if dep.catalogs.is_empty() {
            return Err(dep.outdated());
        }
        let wanted = WantedDependency { alias: name.clone(), bare_specifier: spec.to_string() };
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

pub(super) fn optional_peer_names(manifest: &PackageManifest) -> impl Iterator<Item = &str> {
    manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|meta| meta.iter())
        .filter_map(|(name, entry)| {
            (entry.get("optional").and_then(serde_json::Value::as_bool) == Some(true)).then_some(
                name.as_str(),
            )
        })
}

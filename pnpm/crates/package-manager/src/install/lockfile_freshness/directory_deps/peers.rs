//! The peers of a local directory dependency, as the resolver reads them.

use super::LocalDepContext;
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_catalogs_resolver::{
    CatalogAnchor,
    CatalogResolutionResult,
    WantedDependency,
    resolve_from_catalog,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    PeerSet,
    peer_shadowed_dependency_names,
};
use std::collections::{
    HashMap,
    HashSet,
};

/// How the resolver treats the names a package declares both as one of its
/// own dependencies and as a peer, decided by the resolver's own rule in
/// the scope the package resolved in.
pub(super) struct PeerShadowing<'m> {
    /// The `dependencies` entries the peer edge supplies instead, so the
    /// snapshot records the peer's resolution for them. See
    /// [`peer_shadowed_dependency_names`].
    pub(super) shadowed: HashSet<&'m str>,
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
        PeerShadowing { shadowed }
    }

    /// Compares only the declared peer ranges with the recorded ones. The
    /// resolved peers in the snapshot are whatever the parent provides, such as
    /// a `link:` to a workspace project or a version outside the range (an unmet
    /// peer only warns), so they say nothing about whether the lockfile is stale.
    ///
    /// The lockfile records every [`pnpm_resolving_deps_resolver::declared_peer_dependencies`] entry, and one
    /// written before it did records only the peers the package takes in its
    /// scope, so both are fresh.
    pub(super) fn check_local_peer_deps_freshness(
        &self,
        dep: &LocalDepContext<'_>,
        local_manifest: &PackageManifest,
        pkg_meta: &pnpm_lockfile::PackageMetadata,
    ) -> Result<(), FreshnessCheckError> {
        let peer_set =
            PeerSet::from_manifest(local_manifest.value(), |name| self.shadowed.contains(name));
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

/// A lockfile without a snapshot for a directory peer can still prove that
/// an auto-installed provider falls outside the configured range.
pub(super) fn check_unrecorded_peer_catalogs(
    check: &super::ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &super::scope::AliasScope),
    (manifest, snapshot, metadata): (
        &PackageManifest,
        &pnpm_lockfile::SnapshotEntry,
        &pnpm_lockfile::PackageMetadata,
    ),
) -> Result<(), FreshnessCheckError> {
    let Some(peers) = &metadata.peer_dependencies else { return Ok(()) };
    let taken = std::cell::LazyCell::new(|| {
        let shadowing = PeerShadowing::of(
            manifest,
            |name| scope.contains(name),
            check.config.auto_install_peers,
        );
        PeerSet::from_manifest(manifest.value(), |name| shadowing.shadowed.contains(name))
    });
    let catalog_peers = peers
        .iter()
        .filter_map(|(alias, spec)| {
            Some((alias, pnpm_catalogs_protocol_parser::parse_catalog_protocol(spec)?))
        });
    for (alias, name) in catalog_peers {
        if check.lockfile.catalogs
            .as_ref()
            .and_then(|catalogs| catalogs.get(name))
            .is_some_and(|catalog| catalog.contains_key(alias))
            || !taken.taken().contains_key(alias)
        {
            continue;
        }
        let Some(range) = dep.catalogs
            .get(name)
            .and_then(|catalog| catalog.get(alias))
        else {
            continue;
        };
        if !check.optional_exclusions.frozen_lockfile
            || (check.config.auto_install_peers
                && !scope.contains(alias)
                && !locked_peer_matches_range(snapshot, alias, range))
        {
            return Err(dep.outdated());
        }
    }
    Ok(())
}

fn locked_peer_matches_range(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    alias: &str,
    specifier: &str,
) -> bool {
    let Ok(name) = pnpm_lockfile::PkgName::parse(alias) else { return false };
    let Some(provider) = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .find_map(|deps| deps.get(&name))
        .and_then(|edge| edge.resolve(&name))
    else {
        return true;
    };
    let (target, range) = super::spec::spec_target_and_range(specifier);
    let Some(version) = provider.suffix.registry_version() else { return true };
    if !pnpm_semver_include_prerelease::is_valid_semver_range(range) {
        return true;
    }
    provider.name.to_string() == target.unwrap_or(alias)
        && pnpm_semver_include_prerelease::IncludePrereleaseRange::parse(range).satisfies(version)
}

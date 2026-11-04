use std::collections::{BTreeMap, HashSet};

use pnpm_lockfile::{Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot};
use serde::Serialize;

use crate::snapshot::{resolved_snapshot_version, satisfies, snapshot_dependency};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndeclaredPeer {
    pub package: String,
    pub peer: String,
    pub found_version: String,
}

type Providers = BTreeMap<String, PkgNameVerPeer>;

/// Required peers resolved outside the project's declared provider scope.
/// This is advisory data, separate from missing and incompatible peers.
#[must_use]
pub fn undeclared_peers(
    lockfile: &Lockfile,
    importer_id: &str,
    resolve_from_root: bool,
) -> Vec<UndeclaredPeer> {
    let Some(importer) = lockfile.importers.get(importer_id) else { return Vec::new() };
    let mut providers = Providers::new();
    if resolve_from_root {
        providers.extend(root_providers(lockfile));
    }
    let direct = importer_providers(importer);
    providers.extend(direct.clone());
    let mut pending: Vec<_> = direct
        .into_values()
        .map(|key| (key, providers.clone()))
        .collect();
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    while let Some((key, scope)) = pending.pop() {
        if !visited.insert((key.clone(), scope.clone())) {
            continue;
        }
        inspect_package(lockfile, &key, &scope, &mut result);
        extend_children(lockfile, &key, scope, &mut pending);
    }
    result.sort_by(|left, right| (&left.package, &left.peer).cmp(&(&right.package, &right.peer)));
    result.dedup_by(|left, right| left.package == right.package && left.peer == right.peer);
    result
}

fn root_providers(lockfile: &Lockfile) -> Providers {
    let Some(root) = lockfile.importers.get(".") else { return Providers::new() };
    let declared = importer_providers(root);
    let mut providers = Providers::new();
    for provider in declared.values() {
        providers.entry(provider.name.to_string()).or_insert_with(|| provider.clone());
    }
    providers.extend(declared);
    providers
}

fn importer_providers(importer: &ProjectSnapshot) -> Providers {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|(alias, spec)| {
            spec.version
                .resolved_key(alias)
                .map(|key| (alias.to_string(), key))
        })
        .collect()
}

fn inspect_package(
    lockfile: &Lockfile,
    key: &PkgNameVerPeer,
    scope: &Providers,
    result: &mut Vec<UndeclaredPeer>,
) {
    let Some(packages) = &lockfile.packages else { return };
    let Some(meta) = packages.get(&key.without_peer()) else { return };
    let Some(peers) = &meta.peer_dependencies else { return };
    let Some(snapshot) = lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(key))
    else {
        return;
    };
    let context = PeerContext { key, scope, packages, meta, snapshot };
    result.extend(peers.iter().filter_map(|(peer, range)| inspect_peer(&context, peer, range)));
}

struct PeerContext<'a> {
    key: &'a PkgNameVerPeer,
    scope: &'a Providers,
    packages: &'a std::collections::HashMap<PkgNameVerPeer, pnpm_lockfile::PackageMetadata>,
    meta: &'a pnpm_lockfile::PackageMetadata,
    snapshot: &'a pnpm_lockfile::SnapshotEntry,
}

fn inspect_peer(context: &PeerContext<'_>, peer: &str, range: &str) -> Option<UndeclaredPeer> {
    if context.meta.peer_dependencies_meta
        .as_ref()
        .and_then(|meta| meta.get(peer))
        .is_some_and(|meta| meta.optional)
    {
        return None;
    }
    let alias = peer.parse::<PkgName>().ok()?;
    let reference = snapshot_dependency(context.snapshot, &alias)?;
    let provider = reference.resolve(&alias)?;
    if context.scope.get(peer) == Some(&provider) {
        return None;
    }
    let version =
        resolved_snapshot_version(reference, &alias, context.packages, std::path::Path::new("."))?;
    satisfies(&version, &pnpm_resolving_resolver_base::get_peer_version_range(range))
        .then(|| UndeclaredPeer {
            package: context.key.to_string(),
            peer: peer.to_string(),
            found_version: version,
        })
}

fn extend_children(
    lockfile: &Lockfile,
    key: &PkgNameVerPeer,
    mut scope: Providers,
    pending: &mut Vec<(PkgNameVerPeer, Providers)>,
) {
    let peers = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()))
        .and_then(|meta| meta.peer_dependencies.as_ref());
    let Some(snapshot) = lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(key))
    else {
        return;
    };
    let children: Providers = [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .filter(|(alias, _)| !peers.is_some_and(|peers| peers.contains_key(&alias.to_string())))
        .filter_map(|(alias, reference)| {
            reference
                .resolve(alias)
                .map(|key| (alias.to_string(), key))
        })
        .collect();
    scope.extend(children.clone());
    pending.extend(
        children
            .into_values()
            .map(|child| (child, scope.clone())),
    );
}

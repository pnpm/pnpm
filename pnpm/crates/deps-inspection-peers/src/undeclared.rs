use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use pnpm_lockfile::{Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotDepRef};
use serde::Serialize;

use crate::{
    linked::canonical_path_within,
    snapshot::{child_keys, resolved_snapshot_version, satisfies, snapshot_dependency},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndeclaredPeer {
    pub package: String,
    pub peer: String,
    pub found_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Provider {
    Package(PkgNameVerPeer),
    Link(PathBuf),
}

type Providers = BTreeMap<String, Provider>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Query {
    roots: Vec<PkgNameVerPeer>,
    providers: Providers,
}

struct PackageNode {
    dependencies: Vec<PkgNameVerPeer>,
    children: Providers,
    peers: Vec<(Provider, UndeclaredPeer)>,
}

#[derive(Clone, Copy)]
pub enum WorkspaceRootScope {
    Include,
    Exclude,
}

impl From<bool> for WorkspaceRootScope {
    fn from(enabled: bool) -> Self {
        if enabled { Self::Include } else { Self::Exclude }
    }
}

/// Declaration advisories with shared package analysis and project-scope caching.
pub struct UndeclaredPeerInspector<'a> {
    lockfile: &'a Lockfile,
    lockfile_dir: &'a Path,
    root_scope: Option<Providers>,
    nodes: HashMap<PkgNameVerPeer, PackageNode>,
    reports: HashMap<Query, Vec<UndeclaredPeer>>,
}

impl<'a> UndeclaredPeerInspector<'a> {
    #[must_use]
    pub fn new(lockfile: &'a Lockfile, lockfile_dir: &'a Path) -> Self {
        Self {
            lockfile,
            lockfile_dir,
            root_scope: None,
            nodes: HashMap::new(),
            reports: HashMap::new(),
        }
    }

    /// Inspect required peers independently of missing and incompatible peer errors.
    pub fn check(
        &mut self,
        importer_id: &str,
        root_scope: WorkspaceRootScope,
    ) -> Vec<UndeclaredPeer> {
        let Some(importer) = self.lockfile.importers.get(importer_id) else { return Vec::new() };
        let direct = self.importer_providers(importer, &self.lockfile_dir.join(importer_id));
        let roots = package_keys(&direct).collect();
        let mut providers = Providers::new();
        if matches!(root_scope, WorkspaceRootScope::Include) {
            providers.extend(self.root_providers());
        }
        providers.extend(direct);
        let query = Query { roots, providers };
        if let Some(report) = self.reports.get(&query) {
            return report.clone();
        }
        let reachable = self.reachable_nodes(&query.roots);
        let result = self.inspect_reachable(&query, &reachable);
        self.reports.insert(query, result.clone());
        result
    }

    fn root_providers(&mut self) -> Providers {
        if let Some(providers) = &self.root_scope {
            return providers.clone();
        }
        let Some(root) = self.lockfile.importers.get(".") else { return Providers::new() };
        let declared = self.importer_providers(root, self.lockfile_dir);
        let mut providers = Providers::new();
        for provider in declared.values() {
            if let Some(name) = provider_name(provider) {
                providers.entry(name).or_insert_with(|| provider.clone());
            }
        }
        providers.extend(declared);
        self.root_scope = Some(providers.clone());
        providers
    }

    fn importer_providers(&self, importer: &ProjectSnapshot, base: &Path) -> Providers {
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|(alias, spec)| {
                if let Some(key) = spec.version.resolved_key(alias) {
                    return Some((alias.to_string(), Provider::Package(key)));
                }
                let target = spec.version.as_link_target()?;
                let path = canonical_path_within(&base.join(target), self.lockfile_dir)?;
                Some((alias.to_string(), Provider::Link(path)))
            })
            .collect()
    }

    fn reachable_nodes(&mut self, roots: &[PkgNameVerPeer]) -> HashSet<PkgNameVerPeer> {
        let mut pending = roots.to_vec();
        let mut reachable = HashSet::new();
        while let Some(key) = pending.pop() {
            if !reachable.insert(key.clone()) {
                continue;
            }
            let node = self.nodes
                .entry(key.clone())
                .or_insert_with(|| package_node(self.lockfile, self.lockfile_dir, &key));
            pending.extend(node.dependencies.iter().cloned());
        }
        reachable
    }

    fn inspect_reachable(
        &self,
        query: &Query,
        reachable: &HashSet<PkgNameVerPeer>,
    ) -> Vec<UndeclaredPeer> {
        let mut groups: HashMap<(String, Provider), HashMap<PkgNameVerPeer, UndeclaredPeer>> =
            HashMap::new();
        for key in reachable {
            for (provider, peer) in &self.nodes[key].peers {
                groups
                    .entry((peer.peer.clone(), provider.clone()))
                    .or_default()
                    .insert(key.clone(), peer.clone());
            }
        }
        let mut result = Vec::new();
        for ((peer, provider), targets) in groups {
            result.extend(self.inspect_provider(query, &peer, &provider, &targets));
        }
        result.sort_by(|left, right| {
            (&left.package, &left.peer).cmp(&(&right.package, &right.peer))
        });
        result.dedup_by(|left, right| left.package == right.package && left.peer == right.peer);
        result
    }

    /// Each package has at most two states for one provider, regardless of how paths converge.
    fn inspect_provider(
        &self,
        query: &Query,
        peer: &str,
        provider: &Provider,
        targets: &HashMap<PkgNameVerPeer, UndeclaredPeer>,
    ) -> Vec<UndeclaredPeer> {
        let provided = query.providers.get(peer) == Some(provider);
        let mut pending: Vec<_> = query.roots
            .iter()
            .map(|key| (key, provided))
            .collect();
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        while let Some((key, provided)) = pending.pop() {
            if !visited.insert((key, provided)) {
                continue;
            }
            if !provided && let Some(warning) = targets.get(key) {
                result.push(warning.clone());
            }
            let children = &self.nodes[key].children;
            let provided = children
                .get(peer)
                .map_or(provided, |declared| declared == provider);
            pending.extend(
                self.nodes[key].dependencies
                    .iter()
                    .map(|child| (child, provided)),
            );
        }
        result
    }
}

fn provider_name(provider: &Provider) -> Option<String> {
    match provider {
        Provider::Package(key) => Some(key.name.to_string()),
        Provider::Link(path) => {
            let manifest = pnpm_workspace::read_project_manifest_only(path).ok()?;
            manifest
                .value()
                .get("name")?
                .as_str()
                .map(str::to_string)
        }
    }
}

fn package_key_refs(providers: &Providers) -> impl Iterator<Item = &PkgNameVerPeer> {
    providers
        .values()
        .filter_map(|provider| match provider {
            Provider::Package(key) => Some(key),
            Provider::Link(_) => None,
        })
}

fn package_keys(providers: &Providers) -> impl Iterator<Item = PkgNameVerPeer> {
    package_key_refs(providers).cloned()
}

fn provider_for_reference(
    reference: &SnapshotDepRef,
    alias: &PkgName,
    lockfile_dir: &Path,
) -> Option<Provider> {
    if let Some(key) = reference.resolve(alias) {
        return Some(Provider::Package(key));
    }
    let target = reference.as_link_target()?;
    canonical_path_within(&lockfile_dir.join(target), lockfile_dir).map(Provider::Link)
}

fn package_node(lockfile: &Lockfile, lockfile_dir: &Path, key: &PkgNameVerPeer) -> PackageNode {
    let mut node =
        PackageNode { dependencies: Vec::new(), children: Providers::new(), peers: Vec::new() };
    let Some(packages) = &lockfile.packages else { return node };
    let Some(meta) = packages.get(&key.without_peer()) else { return node };
    let Some(snapshot) = lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(key))
    else {
        return node;
    };
    node.dependencies = child_keys(Some(snapshot)).collect();
    let context = PeerContext { key, packages, meta, snapshot, lockfile_dir };
    node.children = declared_children(&context);
    node.peers = meta.peer_dependencies
        .iter()
        .flatten()
        .filter_map(|(peer, range)| inspect_peer(&context, peer, range))
        .collect();
    node
}

fn declared_children(context: &PeerContext<'_>) -> Providers {
    let mut children = Providers::new();
    for (alias, reference) in
        [&context.snapshot.dependencies, &context.snapshot.optional_dependencies]
            .into_iter()
            .flatten()
            .flatten()
    {
        if !context.meta.peer_dependencies
            .as_ref()
            .is_some_and(|peers| peers.contains_key(&alias.to_string()))
            && let Some(provider) = provider_for_reference(reference, alias, context.lockfile_dir)
        {
            children.insert(alias.to_string(), provider);
        }
    }
    children
}

struct PeerContext<'a> {
    key: &'a PkgNameVerPeer,
    packages: &'a HashMap<PkgNameVerPeer, pnpm_lockfile::PackageMetadata>,
    meta: &'a pnpm_lockfile::PackageMetadata,
    snapshot: &'a pnpm_lockfile::SnapshotEntry,
    lockfile_dir: &'a Path,
}

fn inspect_peer(
    context: &PeerContext<'_>,
    peer: &str,
    range: &str,
) -> Option<(Provider, UndeclaredPeer)> {
    if context.meta.peer_dependencies_meta
        .as_ref()
        .and_then(|meta| meta.get(peer))
        .is_some_and(|meta| meta.optional)
    {
        return None;
    }
    let alias = peer.parse::<PkgName>().ok()?;
    let reference = snapshot_dependency(context.snapshot, &alias)?;
    let provider = provider_for_reference(reference, &alias, context.lockfile_dir)?;
    let version =
        resolved_snapshot_version(reference, &alias, context.packages, context.lockfile_dir)?;
    satisfies(&version, &pnpm_resolving_resolver_base::get_peer_version_range(range))
        .then(|| {
            (
                provider,
                UndeclaredPeer {
                    package: context.key.to_string(),
                    peer: peer.to_string(),
                    found_version: version,
                },
            )
        })
}

#[cfg(test)]
mod tests;

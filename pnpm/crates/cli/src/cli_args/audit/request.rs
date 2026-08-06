//! Turning a lockfile into the dependency graph the registry audits.

use super::{
    BTreeMap, EnvLockfile, HashMap, HashSet, ImporterDepVersion, Lockfile, PackageKey, PkgName,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, SpecifierAndResolution, package_version,
};

#[derive(Debug, Default)]
pub(crate) struct AuditIndexRequest {
    pub(crate) request: BTreeMap<String, Vec<String>>,
    pub(crate) total_dependencies: usize,
    pub(crate) dependencies: usize,
    pub(crate) dev_dependencies: usize,
    pub(crate) optional_dependencies: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Include {
    pub(crate) dependencies: bool,
    pub(crate) dev_dependencies: bool,
    pub(crate) optional_dependencies: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepKind {
    Prod,
    Dev,
    Optional,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub(crate) key: PackageKey,
}

#[derive(Debug)]
pub(crate) struct GraphImporter {
    pub(crate) path_segment: String,
    pub(crate) roots: Vec<(DepKind, Edge)>,
}

#[derive(Debug)]
pub(crate) struct AuditGraph<'a> {
    pub(crate) importers: Vec<GraphImporter>,
    pub(crate) snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
}

pub(crate) fn empty_snapshots() -> &'static HashMap<PackageKey, SnapshotEntry> {
    use std::sync::OnceLock;

    static EMPTY: OnceLock<HashMap<PackageKey, SnapshotEntry>> = OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

pub(crate) fn importer_roots(importer: &pacquet_lockfile::ProjectSnapshot) -> Vec<(DepKind, Edge)> {
    let mut roots = Vec::new();
    append_importer_edges(&mut roots, DepKind::Prod, importer.dependencies.as_ref());
    append_importer_edges(&mut roots, DepKind::Dev, importer.dev_dependencies.as_ref());
    append_importer_edges(&mut roots, DepKind::Optional, importer.optional_dependencies.as_ref());
    roots
}

pub(crate) fn append_importer_edges(
    roots: &mut Vec<(DepKind, Edge)>,
    kind: DepKind,
    deps: Option<&ResolvedDependencyMap>,
) {
    let Some(deps) = deps else { return };
    for (name, spec) in deps {
        if let Some(key) = spec.version.resolved_key(name) {
            roots.push((kind, Edge { key }));
        }
    }
}

pub(crate) fn env_roots(deps: &BTreeMap<String, SpecifierAndResolution>) -> Vec<Edge> {
    deps.iter()
        .filter_map(|(name, spec)| {
            let name = name.parse::<PkgName>().ok()?;
            let version = spec.version.parse::<ImporterDepVersion>().ok()?;
            version.resolved_key(&name).map(|key| Edge { key })
        })
        .collect()
}

pub(crate) fn append_snapshot_edges(
    children: &mut Vec<Edge>,
    deps: Option<&HashMap<PkgName, SnapshotDepRef>>,
) {
    let Some(deps) = deps else { return };
    for (name, dep_ref) in deps {
        if let Some(key) = dep_ref.resolve(name) {
            children.push(Edge { key });
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DepClass {
    pub(crate) dev_only: bool,
    pub(crate) optional_only: bool,
}

pub(crate) fn classify_graph(
    graph: &AuditGraph<'_>,
    include: Include,
) -> HashMap<PackageKey, DepClass> {
    let dev_include = Include {
        dependencies: false,
        dev_dependencies: include.dev_dependencies,
        optional_dependencies: false,
    };
    let non_dev_include = Include {
        dependencies: include.dependencies,
        dev_dependencies: false,
        optional_dependencies: include.optional_dependencies,
    };
    let dev_reachable = walk_reachable(graph, dev_include, include.optional_dependencies);
    let non_dev_reachable = walk_reachable(graph, non_dev_include, include.optional_dependencies);
    let optional_only = collect_optional_only_keys(graph, include);
    dev_reachable
        .union(&non_dev_reachable)
        .cloned()
        .map(|key| {
            let class = DepClass {
                dev_only: dev_reachable.contains(&key) && !non_dev_reachable.contains(&key),
                optional_only: optional_only.contains(&key),
            };
            (key, class)
        })
        .collect()
}

pub(crate) fn collect_optional_only_keys(
    graph: &AuditGraph<'_>,
    include: Include,
) -> HashSet<PackageKey> {
    if !include.optional_dependencies {
        return HashSet::new();
    }
    let with_optional = walk_reachable(graph, include, true);
    let without_optional =
        walk_reachable(graph, Include { optional_dependencies: false, ..include }, false);
    with_optional.difference(&without_optional).cloned().collect()
}

pub(crate) fn walk_reachable(
    graph: &AuditGraph<'_>,
    include: Include,
    include_optional_edges: bool,
) -> HashSet<PackageKey> {
    let mut seen = HashSet::new();
    let mut stack =
        selected_root_edges(graph, include).map(|edge| edge.key.clone()).collect::<Vec<_>>();
    while let Some(key) = stack.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        stack.extend(graph.children(&key, include_optional_edges).into_iter().map(|edge| edge.key));
    }
    seen
}

pub(crate) fn selected_root_edges<'a>(
    graph: &'a AuditGraph<'a>,
    include: Include,
) -> impl Iterator<Item = &'a Edge> {
    graph.importers.iter().flat_map(move |importer| {
        importer
            .roots
            .iter()
            .filter(move |(kind, _)| root_included(*kind, include))
            .map(|(_, edge)| edge)
    })
}

pub(crate) fn root_included(kind: DepKind, include: Include) -> bool {
    match kind {
        DepKind::Prod => include.dependencies,
        DepKind::Dev => include.dev_dependencies,
        DepKind::Optional => include.optional_dependencies,
    }
}

pub(crate) fn lockfile_to_audit_request(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
) -> AuditIndexRequest {
    let mut request = AuditRequestBuilder::default();
    let main = AuditGraph::main(lockfile);
    request.register_graph(&main, include);
    if let Some(env_lockfile) = env_lockfile {
        let env = AuditGraph::env(env_lockfile);
        request.register_graph(&env, include);
    }
    request.finish()
}

#[derive(Default)]
pub(crate) struct AuditRequestBuilder {
    pub(crate) request: BTreeMap<String, Vec<String>>,
    pub(crate) states_by_name: BTreeMap<String, BTreeMap<String, VersionState>>,
    pub(crate) total_dependencies: usize,
    pub(crate) dependencies: usize,
    pub(crate) dev_dependencies: usize,
    pub(crate) optional_dependencies: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VersionState {
    pub(crate) dev_only: bool,
    pub(crate) optional_only: bool,
}

impl AuditRequestBuilder {
    pub(crate) fn register_graph(&mut self, graph: &AuditGraph<'_>, include: Include) {
        let classes = classify_graph(graph, include);
        let mut seen = HashSet::new();
        let mut stack =
            selected_root_edges(graph, include).map(|edge| edge.key.clone()).collect::<Vec<_>>();
        while let Some(key) = stack.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            let class = classes
                .get(&key)
                .copied()
                .unwrap_or(DepClass { dev_only: false, optional_only: false });
            self.register_occurrence(&key, class);
            stack.extend(
                graph
                    .children(&key, include.optional_dependencies)
                    .into_iter()
                    .map(|edge| edge.key),
            );
        }
    }

    pub(crate) fn register_occurrence(&mut self, key: &PackageKey, class: DepClass) {
        let Some(version) = package_version(key) else { return };
        let name = key.name.to_string();
        let version_states = self.states_by_name.entry(name.clone()).or_default();
        let Some(state) = version_states.get_mut(&version) else {
            version_states.insert(
                version.clone(),
                VersionState { dev_only: class.dev_only, optional_only: class.optional_only },
            );
            self.request.entry(name).or_default().push(version);
            self.total_dependencies += 1;
            if class.dev_only {
                self.dev_dependencies += 1;
            }
            if class.optional_only {
                self.optional_dependencies += 1;
            }
            if !class.dev_only && !class.optional_only {
                self.dependencies += 1;
            }
            return;
        };
        let was_production = !state.dev_only && !state.optional_only;
        if state.dev_only && !class.dev_only {
            state.dev_only = false;
            self.dev_dependencies -= 1;
        }
        if state.optional_only && !class.optional_only {
            state.optional_only = false;
            self.optional_dependencies -= 1;
        }
        if !was_production && !state.dev_only && !state.optional_only {
            self.dependencies += 1;
        }
    }

    pub(crate) fn finish(self) -> AuditIndexRequest {
        AuditIndexRequest {
            request: self.request,
            total_dependencies: self.total_dependencies,
            dependencies: self.dependencies,
            dev_dependencies: self.dev_dependencies,
            optional_dependencies: self.optional_dependencies,
        }
    }
}

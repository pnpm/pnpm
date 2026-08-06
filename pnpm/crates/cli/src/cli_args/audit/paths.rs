//! Dependency paths reported alongside each advisory.

use super::{
    AuditGraph, BTreeMap, DepClass, Edge, EnvLockfile, HashMap, HashSet, Include, Lockfile,
    MAX_PATHS_PER_FINDING, PackageKey, Rc, classify_graph, root_included,
};

#[derive(Debug, Default)]
pub(crate) struct PathInfo {
    pub(crate) paths: Vec<String>,
    pub(crate) dev: bool,
    pub(crate) optional: bool,
}

pub(crate) type AuditPathIndex = BTreeMap<String, BTreeMap<String, PathInfo>>;

pub(crate) fn build_audit_path_index(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    vulnerable_names: &HashSet<String>,
    include: Include,
) -> AuditPathIndex {
    let mut paths = AuditPathIndex::default();
    let main = AuditGraph::main(lockfile);
    walk_for_paths(&main, vulnerable_names, include, &mut paths);
    if let Some(env_lockfile) = env_lockfile {
        let env = AuditGraph::env(env_lockfile);
        walk_for_paths(&env, vulnerable_names, include, &mut paths);
    }
    paths
}

#[derive(Debug)]
pub(crate) struct TrailNode {
    pub(crate) name: String,
    pub(crate) parent: Option<Rc<TrailNode>>,
}

#[derive(Debug)]
pub(crate) struct PathFrame {
    pub(crate) key: PackageKey,
    pub(crate) trail: Rc<TrailNode>,
    pub(crate) children: Vec<Edge>,
    pub(crate) next: usize,
}

pub(crate) fn walk_for_paths(
    graph: &AuditGraph<'_>,
    vulnerable_names: &HashSet<String>,
    include: Include,
    paths: &mut AuditPathIndex,
) {
    let classes = classify_graph(graph, include);
    for importer in &graph.importers {
        let importer_trail =
            Rc::new(TrailNode { name: importer.path_segment.clone(), parent: None });
        let mut in_trail = HashSet::new();
        let mut stack: Vec<PathFrame> = Vec::new();
        for (_, root) in importer.roots.iter().filter(|(kind, _)| root_included(*kind, include)) {
            open_path_node(
                graph,
                root.key.clone(),
                Rc::clone(&importer_trail),
                vulnerable_names,
                include,
                &classes,
                paths,
                &mut in_trail,
                &mut stack,
            );
            while let Some(frame) = stack.last_mut() {
                if frame.next < frame.children.len() {
                    let child = frame.children[frame.next].key.clone();
                    let parent = Rc::clone(&frame.trail);
                    frame.next += 1;
                    open_path_node(
                        graph,
                        child,
                        parent,
                        vulnerable_names,
                        include,
                        &classes,
                        paths,
                        &mut in_trail,
                        &mut stack,
                    );
                } else {
                    let frame = stack.pop().expect("stack is non-empty");
                    in_trail.remove(&frame.key);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Path traversal carries independent graph, filter, classification, and output state without a useful grouping abstraction"
)]
pub(crate) fn open_path_node(
    graph: &AuditGraph<'_>,
    key: PackageKey,
    parent_trail: Rc<TrailNode>,
    vulnerable_names: &HashSet<String>,
    include: Include,
    classes: &HashMap<PackageKey, DepClass>,
    paths: &mut AuditPathIndex,
    in_trail: &mut HashSet<PackageKey>,
    stack: &mut Vec<PathFrame>,
) {
    if in_trail.contains(&key) {
        return;
    }
    let name = key.name.to_string();
    let trail = Rc::new(TrailNode { name: name.clone(), parent: Some(parent_trail) });
    if vulnerable_names.contains(&name)
        && let Some(version) = package_version(&key)
    {
        let class = classes
            .get(&key)
            .copied()
            .unwrap_or(DepClass { dev_only: false, optional_only: false });
        record_path(
            paths,
            &name,
            &version,
            join_trail(&trail),
            class.dev_only,
            class.optional_only,
        );
    }
    let children = graph.children(&key, include.optional_dependencies);
    if children.is_empty() {
        return;
    }
    in_trail.insert(key.clone());
    stack.push(PathFrame { key, trail, children, next: 0 });
}

pub(crate) fn record_path(
    paths: &mut AuditPathIndex,
    name: &str,
    version: &str,
    joined: String,
    is_dev: bool,
    is_optional: bool,
) {
    let by_version = paths.entry(name.to_string()).or_default();
    let info = by_version.entry(version.to_string()).or_insert_with(|| PathInfo {
        paths: Vec::new(),
        dev: is_dev,
        optional: is_optional,
    });
    if !is_dev {
        info.dev = false;
    }
    if !is_optional {
        info.optional = false;
    }
    if info.paths.len() >= MAX_PATHS_PER_FINDING || info.paths.contains(&joined) {
        return;
    }
    info.paths.push(joined);
}

pub(crate) fn join_trail(node: &Rc<TrailNode>) -> String {
    let mut parts = Vec::new();
    let mut current = Some(Rc::clone(node));
    while let Some(node) = current.take() {
        parts.push(node.name.clone());
        current.clone_from(&node.parent);
    }
    parts.reverse();
    parts.join(">")
}

pub(crate) fn package_version(key: &PackageKey) -> Option<String> {
    key.suffix.version_semver().map(ToString::to_string)
}

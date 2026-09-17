//! Dependency paths reported alongside each advisory.

mod live_graph;
use live_graph::LiveGraph;

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
    let mut walk = PathWalk::new(graph, vulnerable_names, include, paths);
    for importer in &graph.importers {
        let importer_trail =
            Rc::new(TrailNode { name: importer.path_segment.clone(), parent: None });
        let mut in_trail = HashSet::new();
        let mut stack: Vec<PathFrame> = Vec::new();
        for (_, root) in importer.roots.iter().filter(|(kind, _)| root_included(*kind, include)) {
            open_path_node(
                &mut walk,
                root.key.clone(),
                Rc::clone(&importer_trail),
                paths,
                &mut in_trail,
                &mut stack,
            );
            drain_path_stack(&mut walk, paths, &mut in_trail, &mut stack);
        }
    }
}

/// The inputs one path walk reads for every node it visits.
pub(crate) struct PathWalk<'a> {
    graph: &'a AuditGraph<'a>,
    vulnerable_names: &'a HashSet<String>,
    include: Include,
    classes: HashMap<PackageKey, DepClass>,
    pending: BTreeMap<String, BTreeMap<String, Vec<PackageKey>>>,
    live: LiveGraph,
}

fn finding_saturated(key: &PackageKey, class: DepClass, paths: &AuditPathIndex) -> bool {
    let version = package_version(key).expect("vulnerable target has a version");
    let Some(info) = paths
        .get(&key.name.to_string())
        .and_then(|versions| versions.get(&version))
    else {
        return false;
    };
    info.paths.len() >= MAX_PATHS_PER_FINDING
        && (class.dev_only || !info.dev)
        && (class.optional_only || !info.optional)
}

impl<'a> PathWalk<'a> {
    fn new(
        graph: &'a AuditGraph<'a>,
        vulnerable_names: &'a HashSet<String>,
        include: Include,
        paths: &AuditPathIndex,
    ) -> Self {
        let classes = classify_graph(graph, include);
        let targets = classes
            .iter()
            .filter(|(key, class)| {
                vulnerable_names.contains(&key.name.to_string())
                    && package_version(key).is_some()
                    && !finding_saturated(key, **class, paths)
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let live = LiveGraph::new(graph, include, &classes, &targets);
        let mut pending: BTreeMap<String, BTreeMap<String, Vec<PackageKey>>> = BTreeMap::new();
        for key in targets {
            let version = package_version(&key).expect("vulnerable target has a version");
            pending
                .entry(key.name.to_string())
                .or_default()
                .entry(version)
                .or_default()
                .push(key);
        }
        Self { graph, vulnerable_names, include, classes, pending, live }
    }

    fn prune_saturated_findings(&mut self, name: &str, version: &str, paths: &AuditPathIndex) {
        let targets = self.pending
            .get_mut(name)
            .and_then(|versions| versions.get_mut(version))
            .expect("recorded finding has pending targets");
        for key in std::mem::take(targets) {
            if finding_saturated(&key, self.classes[&key], paths) {
                self.live.remove_target(&key);
            } else {
                targets.push(key);
            }
        }
    }
}

fn drain_path_stack(
    walk: &mut PathWalk<'_>,
    paths: &mut AuditPathIndex,
    in_trail: &mut HashSet<PackageKey>,
    stack: &mut Vec<PathFrame>,
) {
    while let Some(frame) = stack.last_mut() {
        if frame.next >= frame.children.len() {
            let frame = stack.pop().expect("stack is non-empty");
            in_trail.remove(&frame.key);
            continue;
        }
        let child = frame.children[frame.next].key.clone();
        let parent = Rc::clone(&frame.trail);
        frame.next += 1;
        open_path_node(walk, child, parent, paths, in_trail, stack);
    }
}

pub(crate) fn open_path_node(
    walk: &mut PathWalk<'_>,
    key: PackageKey,
    parent_trail: Rc<TrailNode>,
    paths: &mut AuditPathIndex,
    in_trail: &mut HashSet<PackageKey>,
    stack: &mut Vec<PathFrame>,
) {
    if in_trail.contains(&key) || !walk.live.contains(&key) {
        return;
    }
    let name = key.name.to_string();
    let trail = Rc::new(TrailNode { name: name.clone(), parent: Some(parent_trail) });
    if walk.vulnerable_names.contains(&name)
        && let Some(version) = package_version(&key)
    {
        let class = walk.classes
            .get(&key)
            .copied()
            .unwrap_or(DepClass { dev_only: false, optional_only: false });
        if record_path(
            paths,
            &name,
            &version,
            join_trail(&trail),
            class.dev_only,
            class.optional_only,
        ) {
            walk.prune_saturated_findings(&name, &version, paths);
        }
    }
    if !walk.live.contains(&key) {
        return;
    }
    let children = walk.graph.children(&key, walk.include.optional_dependencies);
    if children.is_empty() {
        return;
    }
    in_trail.insert(key.clone());
    stack.push(PathFrame { key, trail, children, next: 0 });
}

/// Returns whether the finding became saturated or its saturated classification changed.
pub(crate) fn record_path(
    paths: &mut AuditPathIndex,
    name: &str,
    version: &str,
    joined: String,
    is_dev: bool,
    is_optional: bool,
) -> bool {
    let by_version = paths.entry(name.to_string()).or_default();
    let info = by_version
        .entry(version.to_string())
        .or_insert_with(|| PathInfo { paths: Vec::new(), dev: is_dev, optional: is_optional });
    let previous_count = info.paths.len();
    let previous_dev = info.dev;
    let previous_optional = info.optional;
    info.dev &= is_dev;
    info.optional &= is_optional;
    if info.paths.len() < MAX_PATHS_PER_FINDING && !info.paths.contains(&joined) {
        info.paths.push(joined);
    }
    info.paths.len() >= MAX_PATHS_PER_FINDING
        && (previous_count < MAX_PATHS_PER_FINDING
            || previous_dev != info.dev
            || previous_optional != info.optional)
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

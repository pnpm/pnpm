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
    let main = AuditGraph::main(lockfile, include.peer_edges);
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
    let mut walk = PathWalk::new(graph, vulnerable_names, include);
    for importer in &graph.importers {
        let importer_trail =
            Rc::new(TrailNode { name: importer.path_segment.clone(), parent: None });
        let mut in_trail = HashSet::new();
        let mut stack: Vec<PathFrame> = Vec::new();
        walk.begin_importer();
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
///
/// Each importer records its first path to a finding even after the finding
/// reached [`MAX_PATHS_PER_FINDING`], so a project whose dependency is shared
/// through many chains cannot hide that another project depends on the same
/// vulnerable package. Targets pruned as saturated are restored when the next
/// importer begins.
pub(crate) struct PathWalk<'a> {
    graph: &'a AuditGraph<'a>,
    vulnerable_names: &'a HashSet<String>,
    include: Include,
    classes: HashMap<PackageKey, DepClass>,
    pending: BTreeMap<String, BTreeMap<String, Vec<PackageKey>>>,
    saturated: Vec<PackageKey>,
    importer_findings: HashSet<(String, String)>,
    live: LiveGraph,
}

fn finding_saturated(
    key: &PackageKey,
    class: DepClass,
    paths: &AuditPathIndex,
    importer_findings: &HashSet<(String, String)>,
) -> bool {
    let version = package_version(key).expect("vulnerable target has a version");
    if !importer_findings.contains(&(key.name.to_string(), version.clone())) {
        return false;
    }
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
    ) -> Self {
        let classes = classify_graph(graph, include);
        let targets = classes
            .keys()
            .filter(|key| {
                vulnerable_names.contains(&key.name.to_string()) && package_version(key).is_some()
            })
            .cloned()
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
        Self {
            graph,
            vulnerable_names,
            include,
            classes,
            pending,
            saturated: Vec::new(),
            importer_findings: HashSet::new(),
            live,
        }
    }

    fn begin_importer(&mut self) {
        self.importer_findings.clear();
        for key in std::mem::take(&mut self.saturated) {
            self.live.add_target(&key);
            let version = package_version(&key).expect("vulnerable target has a version");
            self.pending
                .get_mut(&key.name.to_string())
                .and_then(|versions| versions.get_mut(&version))
                .expect("saturated target has a pending finding")
                .push(key);
        }
    }

    fn prune_saturated_findings(&mut self, name: &str, version: &str, paths: &AuditPathIndex) {
        let targets = self.pending
            .get_mut(name)
            .and_then(|versions| versions.get_mut(version))
            .expect("recorded finding has pending targets");
        for key in std::mem::take(targets) {
            if finding_saturated(&key, self.classes[&key], paths, &self.importer_findings) {
                self.live.remove_target(&key);
                self.saturated.push(key);
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
        let first_from_importer = walk.importer_findings.insert((name.clone(), version.clone()));
        if record_path(
            paths,
            &name,
            &version,
            join_trail(&trail),
            PathClass {
                is_dev: class.dev_only,
                is_optional: class.optional_only,
                exceeds_cap: first_from_importer,
            },
        ) {
            walk.prune_saturated_findings(&name, &version, paths);
        }
    }
    if !walk.live.contains(&key) {
        return;
    }
    let children = walk.graph.children(&key, walk.include);
    if children.is_empty() {
        return;
    }
    in_trail.insert(key.clone());
    stack.push(PathFrame { key, trail, children, next: 0 });
}

/// How a recorded path classifies its finding, and whether it is recorded past
/// [`MAX_PATHS_PER_FINDING`].
pub(crate) struct PathClass {
    pub(crate) is_dev: bool,
    pub(crate) is_optional: bool,
    pub(crate) exceeds_cap: bool,
}

/// Returns whether the finding is saturated and either the path exceeded the cap,
/// the finding just became saturated, or its saturated classification changed.
pub(crate) fn record_path(
    paths: &mut AuditPathIndex,
    name: &str,
    version: &str,
    joined: String,
    PathClass { is_dev, is_optional, exceeds_cap }: PathClass,
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
    if (exceeds_cap || info.paths.len() < MAX_PATHS_PER_FINDING) && !info.paths.contains(&joined) {
        info.paths.push(joined);
    }
    info.paths.len() >= MAX_PATHS_PER_FINDING
        && (exceeds_cap
            || previous_count < MAX_PATHS_PER_FINDING
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

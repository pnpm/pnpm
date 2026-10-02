//! npm's rule for a dependency that fails to resolve inside an optional
//! subtree: the nearest optional dependency above it is left out, together
//! with everything below it. Applying it to the settled tree rather than
//! during the walk keeps the outcome the same whichever occurrence of a
//! shared package walked its children.

use crate::{
    NodeId, ResolveDependencyTreeError, SkippedOptionalDependency, SkippedOptionalDependencyParent,
    SkippedOptionalLogFn,
    resolve_peers::ImporterPeerInput,
    resolved_tree::{ChildEdge, ResolvedPackage, ResolvedTree, TreeChildren},
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// Each importer's `optionalDependencies`, alias to specifier.
pub(super) type OptionalDirect = HashMap<String, String>;

/// Where [`drop_broken_optional_dependencies`] reports what it leaves out.
pub(super) struct SkipReport<'a> {
    pub(super) log: Option<&'a SkippedOptionalLogFn>,
    pub(super) lockfile_dir: &'a Path,
}

/// Drop every optional edge, and every optional direct dependency, that
/// leads to a broken package. A package is broken when the walk recorded it
/// in `broken` or one of its regular dependencies is broken.
///
/// `optional_direct` is in the order of `importers`. A broken package that
/// an importer reaches through a regular dependency fails the install with
/// the recorded error. Returns the packages no importer reaches any more.
pub(super) fn drop_broken_optional_dependencies(
    tree: &mut ResolvedTree,
    importers: &mut [ImporterPeerInput],
    optional_direct: &[OptionalDirect],
    mut broken: HashMap<Arc<str>, ResolveDependencyTreeError>,
    report: &SkipReport<'_>,
) -> Result<HashSet<Arc<str>>, ResolveDependencyTreeError> {
    if broken.is_empty() {
        return Ok(HashSet::default());
    }
    let cause_by_id = propagate_through_regular_edges(&tree.children_by_id, &broken);
    fail_on_regular_path(importers, optional_direct, &cause_by_id, &mut broken)?;
    let details = |id: &Arc<str>| broken[&cause_by_id[id]].to_string();
    for (importer, optional_names) in importers.iter_mut().zip(optional_direct) {
        let root_dir = importer.root_dir.display().to_string();
        importer.direct.retain(|dep| {
            if !cause_by_id.contains_key(&dep.id) {
                return true;
            }
            report.skip_direct(
                &dep.alias,
                &optional_names[&dep.alias],
                &root_dir,
                details(&dep.id),
            );
            false
        });
    }
    let removed = drop_edges_to(tree, &cause_by_id);
    if !removed.is_empty() {
        let first_reached = first_reached(tree, importers);
        for (parent, edge) in removed {
            let origin = report.origin(tree, importers, &first_reached, &parent);
            let details = details(&edge.pkg_id);
            report.skip_nested(tree, origin, &parent, edge, details);
        }
    }
    Ok(left_out_packages(tree, importers, &cause_by_id))
}

/// Fail with the recorded error when an importer reaches a broken package
/// through a regular dependency.
fn fail_on_regular_path(
    importers: &[ImporterPeerInput],
    optional_direct: &[OptionalDirect],
    cause_by_id: &HashMap<Arc<str>, Arc<str>>,
    broken: &mut HashMap<Arc<str>, ResolveDependencyTreeError>,
) -> Result<(), ResolveDependencyTreeError> {
    let required = importers
        .iter()
        .zip(optional_direct)
        .flat_map(|(importer, optional_names)| {
            importer.direct
                .iter()
                .filter(|dep| !optional_names.contains_key(&dep.alias))
        })
        .find_map(|dep| cause_by_id.get(&dep.id));
    match required {
        Some(cause) => Err(broken.remove(cause).expect("every cause is a recorded broken package")),
        None => Ok(()),
    }
}

/// Drop what the left-out packages added to the install's records: their
/// policy violations and their `time:` entries. A violation stays when a
/// package that is still installed has the same name and version.
pub(super) fn forget_left_out(
    tree: &mut ResolvedTree,
    left_out: &HashSet<Arc<str>>,
    mut time: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    if left_out.is_empty() {
        return time;
    }
    let name_version = |pkg: &ResolvedPackage| (pkg.name().to_string(), pkg.version().to_string());
    let (left_out_versions, installed_versions): (HashSet<_>, HashSet<_>) = (
        left_out
            .iter()
            .filter_map(|id| tree.packages.get(id))
            .map(name_version)
            .collect(),
        tree.packages
            .iter()
            .filter(|(id, _)| !left_out.contains(*id))
            .map(|(_, pkg)| name_version(pkg))
            .collect(),
    );
    tree.policy_violations.retain(|violation| {
        let key = (violation.name.to_string(), violation.version.clone());
        !left_out_versions.contains(&key) || installed_versions.contains(&key)
    });
    for pkg in left_out
        .iter()
        .filter_map(|id| tree.packages.get(id))
    {
        time.remove(pkg.result().id.as_str());
    }
    time
}

impl SkipReport<'_> {
    fn skip_direct(&self, alias: &str, specifier: &str, prefix: &str, details: String) {
        self.skip(SkippedOptionalDependency {
            details,
            name: Some(alias.to_string()),
            version: Some(specifier.to_string()),
            bare_specifier: specifier.to_string(),
            parents: Vec::new(),
            prefix: prefix.to_string(),
        });
    }

    fn skip_nested(
        &self,
        tree: &ResolvedTree,
        origin: SkipOrigin,
        parent: &Arc<str>,
        edge: ChildEdge,
        details: String,
    ) {
        let specifier = requested_specifier(tree.packages.get(parent), &edge.alias);
        self.skip(SkippedOptionalDependency {
            details,
            name: Some(edge.alias),
            version: specifier.clone(),
            bare_specifier: specifier.unwrap_or_default(),
            parents: origin.parents,
            prefix: origin.prefix,
        });
    }

    /// The project and the importer-first package chain a kept package is
    /// first reached through, or the lockfile directory and the package
    /// alone when no project reaches it.
    fn origin(
        &self,
        tree: &ResolvedTree,
        importers: &[ImporterPeerInput],
        first_reached: &HashMap<Arc<str>, FirstReach>,
        parent: &Arc<str>,
    ) -> SkipOrigin {
        let Some(reach) = first_reached.get(parent) else {
            return SkipOrigin {
                prefix: self.lockfile_dir.display().to_string(),
                parents: vec![skipped_parent(parent, tree.packages.get(parent))],
            };
        };
        let mut chain = vec![Arc::clone(parent)];
        let mut from = reach.from.as_ref();
        while let Some(id) = from {
            chain.push(Arc::clone(id));
            from = first_reached[id].from.as_ref();
        }
        SkipOrigin {
            prefix: importers[reach.importer].root_dir.display().to_string(),
            parents: chain
                .iter()
                .rev()
                .map(|id| skipped_parent(id, tree.packages.get(id)))
                .collect(),
        }
    }

    fn skip(&self, skipped: SkippedOptionalDependency) {
        if let Some(log) = self.log {
            log(skipped);
        }
    }
}

/// Where a nested skip is reported from. See [`SkipReport::origin`].
struct SkipOrigin {
    prefix: String,
    parents: Vec<SkippedOptionalDependencyParent>,
}

/// How a breadth-first walk from the importers' direct dependencies first
/// reaches a package: through which importer, and from which package.
struct FirstReach {
    importer: usize,
    from: Option<Arc<str>>,
}

fn first_reached(
    tree: &ResolvedTree,
    importers: &[ImporterPeerInput],
) -> HashMap<Arc<str>, FirstReach> {
    let mut walk = FirstReachWalk::default();
    for (importer, input) in importers.iter().enumerate() {
        for dep in &input.direct {
            walk.reach(&dep.id, FirstReach { importer, from: None });
        }
    }
    while let Some(id) = walk.pending.pop_front() {
        let importer = walk.reached[&id].importer;
        for edge in tree.children_by_id
            .get(&id)
            .into_iter()
            .flat_map(|edges| edges.iter())
        {
            walk.reach(&edge.pkg_id, FirstReach { importer, from: Some(Arc::clone(&id)) });
        }
    }
    walk.reached
}

#[derive(Default)]
struct FirstReachWalk {
    reached: HashMap<Arc<str>, FirstReach>,
    pending: std::collections::VecDeque<Arc<str>>,
}

impl FirstReachWalk {
    fn reach(&mut self, id: &Arc<str>, reach: FirstReach) {
        if !self.reached.contains_key(id) {
            self.reached.insert(Arc::clone(id), reach);
            self.pending.push_back(Arc::clone(id));
        }
    }
}

/// The range `parent`'s manifest asks for under `alias`.
fn requested_specifier(parent: Option<&ResolvedPackage>, alias: &str) -> Option<String> {
    let manifest = parent?.result().package.manifest.as_deref()?;
    ["optionalDependencies", "dependencies"]
        .into_iter()
        .find_map(|group| {
            manifest
                .get(group)?
                .get(alias)?
                .as_str()
        })
        .map(ToString::to_string)
}

fn skipped_parent(id: &Arc<str>, pkg: Option<&ResolvedPackage>) -> SkippedOptionalDependencyParent {
    SkippedOptionalDependencyParent {
        id: id.to_string(),
        name: pkg
            .map(|pkg| pkg.name().to_string())
            .unwrap_or_default(),
        version: pkg
            .map(|pkg| pkg.version().to_string())
            .unwrap_or_default(),
    }
}

/// Every broken package, mapped to the recorded package whose failure
/// broke it: the nearest one, the alphabetically first among equals.
fn propagate_through_regular_edges(
    children_by_id: &HashMap<Arc<str>, Arc<Vec<ChildEdge>>>,
    broken: &HashMap<Arc<str>, ResolveDependencyTreeError>,
) -> HashMap<Arc<str>, Arc<str>> {
    let mut regular_parents: HashMap<&str, Vec<&Arc<str>>> = HashMap::default();
    for (parent, edges) in children_by_id {
        for edge in edges.iter().filter(|edge| !edge.optional) {
            regular_parents
                .entry(&edge.pkg_id)
                .or_default()
                .push(parent);
        }
    }
    for parents in regular_parents.values_mut() {
        parents.sort();
    }
    let mut seeds: Vec<Arc<str>> = broken.keys().cloned().collect();
    seeds.sort();
    let mut cause_by_id: HashMap<Arc<str>, Arc<str>> = seeds
        .iter()
        .map(|id| (Arc::clone(id), Arc::clone(id)))
        .collect();
    let mut pending: std::collections::VecDeque<Arc<str>> = seeds.into();
    while let Some(id) = pending.pop_front() {
        let cause = Arc::clone(&cause_by_id[&id]);
        for parent in regular_parents
            .get(&*id)
            .into_iter()
            .flatten()
        {
            if !cause_by_id.contains_key(*parent) {
                cause_by_id.insert(Arc::clone(parent), Arc::clone(&cause));
                pending.push_back(Arc::clone(parent));
            }
        }
    }
    cause_by_id
}

/// Remove the edges into `dropped` from the recorded children lists and
/// from every realized occurrence, returning each removed edge with its
/// parent. Only optional edges can be left pointing there: a regular edge
/// makes its parent broken too.
fn drop_edges_to(
    tree: &mut ResolvedTree,
    dropped: &HashMap<Arc<str>, Arc<str>>,
) -> Vec<(Arc<str>, ChildEdge)> {
    drop_realized_children(tree, dropped);
    let mut removed = Vec::new();
    for (parent, edges) in &mut tree.children_by_id {
        if dropped.contains_key(parent) {
            continue;
        }
        let (into_dropped, kept): (Vec<ChildEdge>, Vec<ChildEdge>) = edges
            .iter()
            .cloned()
            .partition(|edge| dropped.contains_key(&edge.pkg_id));
        if !into_dropped.is_empty() {
            *edges = Arc::new(kept);
            removed.extend(
                into_dropped
                    .into_iter()
                    .map(|edge| (Arc::clone(parent), edge)),
            );
        }
    }
    removed.sort_by(|left, right| (&left.0, &left.1.alias).cmp(&(&right.0, &right.1.alias)));
    removed
}

fn drop_realized_children(tree: &mut ResolvedTree, dropped: &HashMap<Arc<str>, Arc<str>>) {
    let dropped_nodes: HashSet<NodeId> = tree.dependencies_tree
        .iter()
        .filter(|(_, node)| dropped.contains_key(&node.resolved_package_id))
        .map(|(node_id, _)| node_id.clone())
        .collect();
    for node in tree.dependencies_tree.values_mut() {
        if let TreeChildren::Realized(children) = &mut node.children
            && children
                .values()
                .any(|child| dropped_nodes.contains(child))
        {
            *children = Arc::new(
                children
                    .iter()
                    .filter(|(_, child)| !dropped_nodes.contains(*child))
                    .map(|(alias, child)| (alias.clone(), child.clone()))
                    .collect(),
            );
        }
    }
}

/// The packages below a broken one that no importer reaches any more.
fn left_out_packages(
    tree: &ResolvedTree,
    importers: &[ImporterPeerInput],
    broken: &HashMap<Arc<str>, Arc<str>>,
) -> HashSet<Arc<str>> {
    let mut below_broken = reachable(tree, broken.keys().cloned());
    let still_reached = reachable(
        tree,
        importers
            .iter()
            .flat_map(|importer| importer.direct.iter())
            .map(|dep| Arc::clone(&dep.id)),
    );
    below_broken.retain(|id| !still_reached.contains(id));
    below_broken
}

fn reachable(tree: &ResolvedTree, roots: impl Iterator<Item = Arc<str>>) -> HashSet<Arc<str>> {
    let mut seen: HashSet<Arc<str>> = HashSet::default();
    let mut pending: Vec<Arc<str>> = roots.collect();
    while let Some(id) = pending.pop() {
        if !seen.insert(Arc::clone(&id)) {
            continue;
        }
        for edge in tree.children_by_id
            .get(&id)
            .into_iter()
            .flat_map(|edges| edges.iter())
        {
            pending.push(Arc::clone(&edge.pkg_id));
        }
    }
    seen
}

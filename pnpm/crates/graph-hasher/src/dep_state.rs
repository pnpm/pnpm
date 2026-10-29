use crate::{
    hash_object,
    object_hasher::{digest_base64, serialize_str},
};
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX: &str = "dependency-side-effects:v1:";

/// Per-node identifier carrying everything [`calc_dep_state`] needs to
/// hash a snapshot.
///
/// `full_pkg_id` is the fingerprint used as the `id` field in the
/// recursive hash — `<pkgIdWithPatchHash>:<integrity>` for packages with
/// an integrity (`registry` resolution), or
/// `<pkgIdWithPatchHash>:<hashObject(resolution)>` for resolutions
/// without one (e.g. git refs). A variations resolution uses the integrity of
/// the selected platform variant. Pacquet's caller composes this before
/// passing it in; the hasher itself is opaque to how it was computed.
///
/// `children` maps alias → dep-graph key for the snapshot's
/// children. Pacquet's natural input shape is the lockfile's
/// `snapshots[].dependencies` + `optionalDependencies` flattened,
/// with each value resolved to the snapshot key it points at.
///
/// Owns its strings so a caller building the graph from a lockfile
/// doesn't have to keep a separate `String` arena alive for the
/// duration of the hash walk.
pub struct DepsGraphNode<Key> {
    pub full_pkg_id: String,
    pub children: IndexMap<String, Key>,
}

/// Memoized per-depPath state cache: the result of `hash_object` for
/// each visited node is stashed so the recursive walk over diamond-shaped
/// graphs stays linear.
pub type DepsStateCache<Key> = HashMap<Key, String>;

/// Inputs to [`calc_dep_state`].
pub struct CalcDepStateOptions<'a> {
    /// Output of [`crate::engine_name()`] — the platform / arch /
    /// node version prefix. Always part of the result.
    pub engine_name: &'a str,
    /// SHA-256 hex of the patch file for this package (when present).
    /// Appended as `;patch=<hash>`.
    pub patch_file_hash: Option<&'a str>,
    /// Whether to include the recursive dep-graph hash as
    /// `;deps=<hash>`. Set to `hasSideEffects`
    /// (i.e. `!ignoreScripts && requiresBuild`).
    pub include_dep_graph_hash: bool,
}

/// The side-effects diff format [`calc_dep_state`] names in the cache key.
/// Format 2 records the symlinks a build creates, which a pnpm version
/// reading format 1 would restore as regular files, so the two formats are
/// kept under separate keys.
pub const SIDE_EFFECTS_FORMAT_KEY: &str = "format=2";

/// Compute the side-effects cache key for a snapshot.
///
/// Returns the cache key for the side-effects cache. Format:
/// `<engine_name>;format=2[;deps=<hash>][;patch=<hash>]`.
pub fn calc_dep_state<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    dep_path: &Key,
    opts: &CalcDepStateOptions<'_>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    let mut result = format!("{};{SIDE_EFFECTS_FORMAT_KEY}", opts.engine_name);
    if opts.include_dep_graph_hash {
        let deps_hash = calc_dep_graph_hash(graph, cache, dep_path);
        result.push_str(";deps=");
        result.push_str(&deps_hash);
    }
    if let Some(patch) = opts.patch_file_hash {
        result.push_str(";patch=");
        result.push_str(patch);
    }
    result
}

/// Compute the machine-independent lookup key for a remotely shareable
/// dependency build.
///
/// Unlike [`calc_dep_state`], this key deliberately excludes the engine name.
/// The signed artifact advertises platform compatibility separately, allowing
/// one compatible artifact to serve more than one exact host identity.
///
/// `graph` must contain `dep_path`; a missing root is a caller error and
/// panics rather than producing the same key for every missing dependency.
/// The walk uses a private per-call cache, so the result is independent of
/// earlier roots and platform-selected graphs. `graph` is not mutated.
///
/// The returned key starts with [`DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX`],
/// followed by the recursive dependency-graph hash and, when supplied, the
/// patch-file hash. The selected variation's source integrity is included in
/// the graph hash through [`DepsGraphNode::full_pkg_id`].
pub fn calc_dep_state_input_key<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    dep_path: &Key,
    patch_file_hash: Option<&str>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    assert!(
        graph.contains_key(dep_path),
        "dependency side-effects input-key root is not present in the graph",
    );
    let deps_hash = calc_dep_graph_hash(graph, &mut HashMap::new(), dep_path);
    let mut result = format!("{DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX}deps={deps_hash}");
    if let Some(patch) = patch_file_hash {
        result.push_str(";patch=");
        result.push_str(patch);
    }
    result
}

/// The `deps=` portion: the digest of the subgraph `dep_path` reaches.
///
/// A node outside any dependency cycle hashes as
/// `hash_object({ id, deps })`, where `deps` maps each alias to the
/// child's digest. A child the graph has no node for contributes `""`.
///
/// The members of a cycle (a strongly connected component, or a node
/// that depends on itself) hash as one unit, so each member's digest
/// covers everything the cycle reaches.
///
/// The digests depend only on the graph, not on the order nodes or
/// children are visited in, so a shared `cache` gives the same answers
/// whichever root reaches a node first.
pub(crate) fn calc_dep_graph_hash<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    dep_path: &Key,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    if let Some(cached) = cache.get(dep_path) {
        return cached.clone();
    }
    let Some((dep_path, _)) = graph.get_key_value(dep_path) else {
        return String::new();
    };
    let mut walk = ComponentWalk {
        graph,
        cache,
        visits: HashMap::new(),
        stack: Vec::new(),
        scratch: Vec::with_capacity(8192),
    };
    walk.visit(dep_path);
    cache[dep_path].clone()
}

/// Tarjan's strongly-connected-components search over the nodes that
/// are not in `cache` yet. It finishes a component only after every
/// component the component depends on, so a component's external
/// children are always hashed by the time it is.
struct ComponentWalk<'graph, 'cache, Key> {
    graph: &'graph HashMap<Key, DepsGraphNode<Key>>,
    cache: &'cache mut DepsStateCache<Key>,
    visits: HashMap<&'graph Key, Visit>,
    stack: Vec<&'graph Key>,
    /// Reused by every [`digest_node`] of the walk.
    scratch: Vec<u8>,
}

struct Visit {
    index: usize,
    low_link: usize,
    on_stack: bool,
}

impl<'graph, Key> ComponentWalk<'graph, '_, Key>
where
    Key: Clone + Eq + std::hash::Hash,
{
    fn visit(&mut self, dep_path: &'graph Key) {
        let index = self.visits.len();
        self.visits.insert(dep_path, Visit { index, low_link: index, on_stack: true });
        self.stack.push(dep_path);
        for child_key in self.graph[dep_path].children.values() {
            if let Some(reached) = self.visit_child(child_key) {
                let visit = self.visits.get_mut(dep_path).expect("visited above");
                visit.low_link = visit.low_link.min(reached);
            }
        }
        if self.visits[dep_path].low_link == index {
            self.finish_component(dep_path);
        }
    }

    /// The lowest index `child_key` reaches back to, or `None` for a
    /// child that is missing from the graph or in a finished component.
    fn visit_child(&mut self, child_key: &Key) -> Option<usize> {
        let (child_key, _) = self.graph.get_key_value(child_key)?;
        if self.cache.contains_key(child_key) {
            return None;
        }
        match self.visits.get(child_key) {
            None => {
                self.visit(child_key);
                Some(self.visits[child_key].low_link)
            }
            Some(child) => child.on_stack.then_some(child.index),
        }
    }

    fn finish_component(&mut self, root: &'graph Key) {
        let start = self.stack
            .iter()
            .rposition(|member| *member == root)
            .expect("a visited node stays on the stack until its component is finished");
        let members = self.stack.split_off(start);
        for member in &members {
            self.visits.get_mut(*member).expect("visited above").on_stack = false;
        }
        self.hash_component(&members);
    }

    fn hash_component(&mut self, members: &[&'graph Key]) {
        if let [dep_path] = members {
            let node = &self.graph[*dep_path];
            if !node.children
                .values()
                .any(|child_key| child_key == *dep_path)
            {
                let hashed = digest_node(node, self.cache, &mut self.scratch);
                self.cache.insert((*dep_path).clone(), hashed);
                return;
            }
        }
        let hashes = hash_cycle(self.graph, self.cache, members);
        for (member, hashed) in members.iter().zip(hashes) {
            self.cache.insert((*member).clone(), hashed);
        }
    }
}

/// The digests of the members of one dependency cycle, in `members`
/// order. Every child outside the cycle must already be in `cache`.
///
/// Each member is first labelled `hash_object({ id, deps })` over its
/// children outside the cycle. The labels are then refined with the
/// children inside the cycle,
/// `hash_object({ color: <label>, deps: { <alias>: <child label> } })`,
/// until a round no longer splits a group of equal labels. Two members
/// end with the same label only when their dependency trees are the
/// same, however far they are unrolled.
///
/// The cycle's `shape` maps each final label to that member's
/// `{ <alias>: <child label> }` inside the cycle. A member's digest is
/// `hash_object({ member: <label>, component: hash_object(shape) })`,
/// so it changes when anything in or below the cycle changes.
fn hash_cycle<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &DepsStateCache<Key>,
    members: &[&Key],
) -> Vec<String>
where
    Key: Clone + Eq + std::hash::Hash,
{
    let cycle = Cycle {
        graph,
        members,
        position: members
            .iter()
            .enumerate()
            .map(|(index, member)| (*member, index))
            .collect(),
    };
    let labels = refine_labels(cycle.initial_labels(cache), &|labels, index| {
        cycle.internal_deps(index, labels)
    });
    let shape: serde_json::Map<String, Value> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| (label.clone(), cycle.internal_deps(index, &labels)))
        .collect();
    let component = hash_object(&Value::Object(shape));
    labels
        .iter()
        .map(|label| hash_object(&json!({ "member": label, "component": component })))
        .collect()
}

/// The members of one dependency cycle, for [`hash_cycle`].
struct Cycle<'a, Key> {
    graph: &'a HashMap<Key, DepsGraphNode<Key>>,
    members: &'a [&'a Key],
    position: HashMap<&'a Key, usize>,
}

impl<Key> Cycle<'_, Key>
where
    Key: Clone + Eq + std::hash::Hash,
{
    /// Each member's `hash_object({ id, deps })` over its children
    /// outside the cycle.
    fn initial_labels(&self, cache: &DepsStateCache<Key>) -> Vec<String> {
        self.members
            .iter()
            .map(|member| {
                let node = &self.graph[*member];
                let external_deps = node.children
                    .iter()
                    .filter(|(_, child_key)| !self.position.contains_key(child_key))
                    .map(|(alias, child_key)| {
                        let child = cache
                            .get(child_key)
                            .map(String::as_str)
                            .unwrap_or("");
                        (alias.clone(), Value::String(child.to_owned()))
                    })
                    .collect();
                hash_object(
                    &json!({ "id": node.full_pkg_id, "deps": Value::Object(external_deps) }),
                )
            })
            .collect()
    }

    /// `{ <alias>: <child label> }` over the children of the member at
    /// `index` that are inside the cycle.
    fn internal_deps(&self, index: usize, labels: &[String]) -> Value {
        let deps = self.graph[self.members[index]].children
            .iter()
            .filter_map(|(alias, child_key)| {
                let child = self.position.get(child_key)?;
                Some((alias.clone(), Value::String(labels[*child].clone())))
            })
            .collect();
        Value::Object(deps)
    }
}

/// Refine each member's label with its children's labels until a round
/// no longer splits a group of equal labels.
fn refine_labels(
    mut labels: Vec<String>,
    internal_deps: &dyn Fn(&[String], usize) -> Value,
) -> Vec<String> {
    let mut groups = count_distinct(&labels);
    loop {
        let refined: Vec<String> = labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                hash_object(&json!({ "color": label, "deps": internal_deps(&labels, index) }))
            })
            .collect();
        let refined_groups = count_distinct(&refined);
        if refined_groups == groups {
            return labels;
        }
        labels = refined;
        groups = refined_groups;
    }
}

fn count_distinct(labels: &[String]) -> usize {
    labels
        .iter()
        .collect::<HashSet<_>>()
        .len()
}

/// The digest of one node outside any cycle, written into `scratch`
/// without an intermediate `serde_json` value. Every child must already
/// be in `cache`.
///
/// The bytes are the object-hash serialization of
/// `{ "id": <full_pkg_id>, "deps": { <alias>: <child digest>, .. } }`
/// with `sort = true`: an `object:2:` header whose sorted keys put
/// `deps` before `id`, each pair framed `<key>:<value>,`. Sorting the
/// aliases reproduces the sort `crate::object_hasher::serialize` runs
/// over an object's keys, which is what keeps the two byte-identical.
fn digest_node<Key>(
    node: &DepsGraphNode<Key>,
    cache: &DepsStateCache<Key>,
    scratch: &mut Vec<u8>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    // A child the graph has no node for contributes the empty string.
    let mut pairs: Vec<(&str, &str)> = node.children
        .iter()
        .map(|(alias, child_key)| {
            (
                alias.as_str(),
                cache
                    .get(child_key)
                    .map(String::as_str)
                    .unwrap_or(""),
            )
        })
        .collect();
    pairs.sort_unstable_by_key(|(alias, _)| *alias);

    scratch.clear();
    scratch.extend_from_slice(b"object:2:");
    serialize_str(scratch, "deps");
    scratch.push(b':');
    scratch.extend_from_slice(b"object:");
    scratch.extend_from_slice(pairs.len().to_string().as_bytes());
    scratch.push(b':');
    for (alias, child_digest) in &pairs {
        serialize_str(scratch, alias);
        scratch.push(b':');
        serialize_str(scratch, child_digest);
        scratch.push(b',');
    }
    scratch.push(b',');
    serialize_str(scratch, "id");
    scratch.push(b':');
    serialize_str(scratch, &node.full_pkg_id);
    scratch.push(b',');
    digest_base64(scratch)
}

/// Return every node that is, or transitively depends on, a node
/// in `built_dep_paths`.
///
/// The result controls whether [`crate::calc_graph_node_hash`] includes
/// the engine in a global-virtual-store hash. It is computed as one
/// graph-wide fixed point so dependency cycles cannot produce different
/// answers for different entry points.
///
/// A key with no node in `graph` is kept and still marks whatever depends
/// on it: the built set comes from the allow-build policy rather than the
/// graph, so the two can disagree.
#[must_use]
pub fn build_required_dep_paths<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    built_dep_paths: &HashSet<Key>,
) -> HashSet<Key>
where
    Key: Clone + Eq + std::hash::Hash,
{
    if built_dep_paths.is_empty() {
        return HashSet::new();
    }

    let parents_by_child = index_parents_by_child(graph);

    let mut build_required: HashSet<&Key> = built_dep_paths.iter().collect();
    let mut pending: Vec<&Key> = built_dep_paths.iter().collect();
    while let Some(child) = pending.pop() {
        let Some(parents) = parents_by_child.get(child) else {
            continue;
        };
        for parent in parents {
            if build_required.insert(parent) {
                pending.push(parent);
            }
        }
    }
    build_required
        .into_iter()
        .cloned()
        .collect()
}

/// Reverse the graph: every node, and the nodes that depend on it.
fn index_parents_by_child<Key>(graph: &HashMap<Key, DepsGraphNode<Key>>) -> HashMap<&Key, Vec<&Key>>
where
    Key: Eq + std::hash::Hash,
{
    let mut parents_by_child: HashMap<&Key, Vec<&Key>> = HashMap::new();
    for (parent, node) in graph {
        for child in node.children.values() {
            parents_by_child
                .entry(child)
                .or_default()
                .push(parent);
        }
    }
    parents_by_child
}

#[cfg(test)]
mod tests;

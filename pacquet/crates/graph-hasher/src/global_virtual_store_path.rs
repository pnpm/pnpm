//! Pacquet port of pnpm's global-virtual-store directory naming —
//! [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L122-L146)
//! and
//! [`formatGlobalVirtualStorePath`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L155-L160).
//!
//! The engine string contribution is gated by
//! `transitively_requires_build` (private to this crate): when the
//! caller supplies a `built_dep_paths` set (derived from
//! `allowBuilds` / `dangerouslyAllowAllBuilds`), only snapshots
//! that themselves run a build script — or that transitively
//! depend on one — keep the engine in the GVS hash payload.
//! Pure-JS leaves and their pure-JS ancestors hash with
//! `engine = null`, so their GVS directories survive Node.js
//! upgrades and architecture moves. Passing `None` for
//! `built_dep_paths` reproduces the always-include behaviour
//! (matches upstream's
//! [`builtDepPaths === undefined`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L140-L142)
//! branch).

use crate::{
    HashEncoding,
    dep_state::{calc_dep_graph_hash, transitively_requires_build},
    hash_object_without_sorting,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::dep_state::{DepsGraphNode, DepsStateCache};

/// Compute the hex digest that uniquely identifies one snapshot's
/// position in the global virtual store. Mirrors
/// [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L122-L146).
///
/// The output is the `hash` segment that
/// [`format_global_virtual_store_path`] places after `<name>/<version>/`.
/// Two snapshots that resolve to the same package contents (identical
/// `fullPkgId`s and identical recursive children) hash to the same
/// value and therefore share one directory under
/// `<store>/links/<name>/<version>/` — which is how pnpm and pacquet
/// avoid re-extracting the same tarball once per peer-context.
///
/// `engine` is the candidate `ENGINE_NAME`-shaped string
/// (`<platform>-<arch>-node<major>`). It is included in the hash
/// payload only when the snapshot transitively requires a build —
/// pure-JS subgraphs hash with `engine = null` so their GVS
/// directories survive Node.js upgrades. The gating is driven by:
///
/// - `built_dep_paths`: when `Some`, the set of snapshot keys whose
///   `allowBuilds` entry evaluates to `true` (or every snapshot when
///   `dangerouslyAllowAllBuilds` is on). `None` disables the gating
///   and forces `include_engine = true` — matches upstream's
///   `builtDepPaths === undefined` branch.
/// - `build_required_cache`: install-scoped memoization for the
///   gating walk. Allocated once at the call site and reused across
///   every snapshot in the install — see
///   [`iterateHashedGraphNodes`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L99-L120)
///   for the upstream lifecycle. Untouched when `built_dep_paths`
///   is `None`; callers that don't care can hold a throwaway
///   `let mut cache = HashMap::new();` and pass `&mut cache`.
pub fn calc_graph_node_hash<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    dep_path: &Key,
    engine: Option<&str>,
    built_dep_paths: Option<&HashSet<Key>>,
    build_required_cache: &mut HashMap<Key, bool>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    let include_engine = match built_dep_paths {
        None => true,
        Some(set) => transitively_requires_build(
            graph,
            set,
            build_required_cache,
            dep_path,
            &mut HashSet::new(),
        ),
    };
    let engine_value = if include_engine {
        match engine {
            Some(s) => Value::String(s.to_owned()),
            None => Value::Null,
        }
    } else {
        Value::Null
    };
    let deps_hash = calc_dep_graph_hash(graph, cache, &mut HashSet::new(), dep_path);
    let payload = json!({
        "engine": engine_value,
        "deps": deps_hash,
    });
    hash_object_without_sorting(&payload, HashEncoding::Hex)
}

/// Format a global-virtual-store-relative path for a package. Mirrors
/// upstream's
/// [`formatGlobalVirtualStorePath`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L155-L160)
/// — the `@/` prefix on unscoped packages keeps every entry in the
/// shared store at the same `<scope>/<name>/<version>/<hash>` depth,
/// so a single `readdir` pass per level can enumerate the store
/// without special-casing the unscoped path layout.
pub fn format_global_virtual_store_path(name: &str, version: &str, hex_digest: &str) -> String {
    let prefix = if name.starts_with('@') { "" } else { "@/" };
    format!("{prefix}{name}/{version}/{hex_digest}")
}

#[cfg(test)]
mod tests {
    use super::{calc_graph_node_hash, format_global_virtual_store_path};
    use crate::dep_state::DepsGraphNode;
    use std::collections::{HashMap, HashSet};

    /// Scoped packages don't get the `@/` prefix — they already start
    /// with `@<scope>/`. Unscoped packages do.
    #[test]
    fn format_prefixes_unscoped_with_at_slash() {
        assert_eq!(
            format_global_virtual_store_path("foo", "1.2.3", "deadbeef"),
            "@/foo/1.2.3/deadbeef",
        );
        assert_eq!(
            format_global_virtual_store_path("@scope/foo", "1.2.3", "deadbeef"),
            "@scope/foo/1.2.3/deadbeef",
        );
    }

    /// Two graphs with identical structure and `full_pkg_id`s produce
    /// the same hash — same as upstream's design where the GVS path is
    /// the deduplication key.
    #[test]
    fn identical_leaves_hash_identically() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache_a = HashMap::new();
        let mut cache_b = HashMap::new();
        let mut br_a = HashMap::new();
        let mut br_b = HashMap::new();
        let a = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"leaf@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br_a,
        );
        let b = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"leaf@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br_b,
        );
        assert_eq!(a, b, "deterministic for same input");
        assert_eq!(a.len(), 64, "sha256 hex digest is 64 chars");
    }

    /// Different engines produce different hashes when the engine
    /// is included. Mirrors upstream's contribution of `ENGINE_NAME`
    /// (vs `null`) to the hash payload at
    /// `deps/graph-hasher/src/index.ts:140-146`.
    #[test]
    fn engine_string_changes_hash() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache = HashMap::new();
        let mut br = HashMap::new();
        let with_engine = calc_graph_node_hash(
            &graph,
            &mut cache,
            &"leaf@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br,
        );
        let mut cache_other = HashMap::new();
        let mut br_other = HashMap::new();
        let with_other_engine = calc_graph_node_hash(
            &graph,
            &mut cache_other,
            &"leaf@1.0.0".to_string(),
            Some("linux-x64-node22"),
            None,
            &mut br_other,
        );
        let mut cache_null = HashMap::new();
        let mut br_null = HashMap::new();
        let with_null = calc_graph_node_hash(
            &graph,
            &mut cache_null,
            &"leaf@1.0.0".to_string(),
            None,
            None,
            &mut br_null,
        );
        assert_ne!(with_engine, with_other_engine);
        assert_ne!(with_engine, with_null);
        assert_ne!(with_other_engine, with_null);
    }

    /// Engine-agnostic gating: when `built_dep_paths` excludes the
    /// snapshot and its subtree, two different `engine` strings
    /// hash to the *same* digest. This is the whole point of the
    /// `transitivelyRequiresBuild` gating — pure-JS leaves survive
    /// Node.js upgrades because their GVS hash drops the engine
    /// contribution.
    #[test]
    fn engine_agnostic_when_subtree_has_no_builders() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "pure-js@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "pure-js@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        // builtDepPaths exists but doesn't contain pure-js. Gating fires.
        let built: HashSet<String> = ["someone-else@1.0.0".to_string()].into_iter().collect();
        let mut cache_a = HashMap::new();
        let mut br_a = HashMap::new();
        let darwin = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"pure-js@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            Some(&built),
            &mut br_a,
        );
        let mut cache_b = HashMap::new();
        let mut br_b = HashMap::new();
        let linux = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"pure-js@1.0.0".to_string(),
            Some("linux-x64-node22"),
            Some(&built),
            &mut br_b,
        );
        assert_eq!(
            darwin, linux,
            "pure-js subtree must hash engine-agnostically when gated by builtDepPaths",
        );
    }

    /// Gating's positive case: when the snapshot itself is in
    /// `built_dep_paths`, the engine *is* included — two different
    /// engines diverge. Symmetric with [`engine_agnostic_when_subtree_has_no_builders`].
    #[test]
    fn engine_included_when_self_in_built_set() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "native@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "native@1.0.0:sha512-n".to_string(),
                children: HashMap::new(),
            },
        );
        let built: HashSet<String> = ["native@1.0.0".to_string()].into_iter().collect();
        let mut cache_a = HashMap::new();
        let mut br_a = HashMap::new();
        let darwin = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"native@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            Some(&built),
            &mut br_a,
        );
        let mut cache_b = HashMap::new();
        let mut br_b = HashMap::new();
        let linux = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"native@1.0.0".to_string(),
            Some("linux-x64-node22"),
            Some(&built),
            &mut br_b,
        );
        assert_ne!(darwin, linux, "builder must partition by engine string");
    }

    /// Gating's transitive case: a non-builder snapshot whose
    /// child *is* a builder also includes the engine. Mirrors
    /// upstream's recursion at index.ts:241-245.
    #[test]
    fn engine_included_for_ancestor_of_builder() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut root_children = HashMap::new();
        root_children.insert("dep".to_string(), "native@1.0.0".to_string());
        graph.insert(
            "root@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-r".to_string(),
                children: root_children,
            },
        );
        graph.insert(
            "native@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "native@1.0.0:sha512-n".to_string(),
                children: HashMap::new(),
            },
        );
        let built: HashSet<String> = ["native@1.0.0".to_string()].into_iter().collect();
        let mut cache_a = HashMap::new();
        let mut br_a = HashMap::new();
        let darwin = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"root@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            Some(&built),
            &mut br_a,
        );
        let mut cache_b = HashMap::new();
        let mut br_b = HashMap::new();
        let linux = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"root@1.0.0".to_string(),
            Some("linux-x64-node22"),
            Some(&built),
            &mut br_b,
        );
        assert_ne!(darwin, linux, "ancestor of a builder must partition by engine string");
    }

    /// `built_dep_paths = None` reproduces the always-include-engine
    /// behaviour: two different engines hash differently even for a
    /// snapshot that no builder reaches. Mirrors upstream's
    /// `builtDepPaths === undefined || ...` short-circuit at
    /// index.ts:140.
    #[test]
    fn none_built_dep_paths_disables_gating() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "pure-js@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "pure-js@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache_a = HashMap::new();
        let mut br_a = HashMap::new();
        let darwin = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"pure-js@1.0.0".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br_a,
        );
        let mut cache_b = HashMap::new();
        let mut br_b = HashMap::new();
        let linux = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"pure-js@1.0.0".to_string(),
            Some("linux-x64-node22"),
            None,
            &mut br_b,
        );
        assert_ne!(
            darwin, linux,
            "without builtDepPaths gating, engine is always part of the hash",
        );
    }

    /// Two snapshots whose children differ (same `full_pkg_id`,
    /// different deps) hash differently — the GVS path includes the
    /// transitive dep contribution.
    #[test]
    fn different_children_change_hash() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let mut root_a_children = HashMap::new();
        root_a_children.insert("a".to_string(), "leaf@1.0.0".to_string());
        graph.insert(
            "root@1.0.0(a)".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-r".to_string(),
                children: root_a_children,
            },
        );
        graph.insert(
            "root@1.0.0(b)".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-r".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache_a = HashMap::new();
        let mut br_a = HashMap::new();
        let with_dep = calc_graph_node_hash(
            &graph,
            &mut cache_a,
            &"root@1.0.0(a)".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br_a,
        );
        let mut cache_b = HashMap::new();
        let mut br_b = HashMap::new();
        let without_dep = calc_graph_node_hash(
            &graph,
            &mut cache_b,
            &"root@1.0.0(b)".to_string(),
            Some("darwin-arm64-node20"),
            None,
            &mut br_b,
        );
        assert_ne!(
            with_dep, without_dep,
            "same root, different children must not collide on GVS hash",
        );
    }
}

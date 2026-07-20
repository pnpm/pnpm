//! Dependency-tree building shared by `pnpm list` and `pnpm why`.
//!
//! Rust counterpart of the TypeScript `@pnpm/deps.inspection.tree-builder`
//! package: a lockfile-backed dependency graph ([`graph`]), a
//! materializer that turns it into renderable [`DependencyNode`] trees
//! with deduplication and circular-reference marking ([`get_tree`]),
//! per-node metadata resolution ([`pkg_info`]), dev/prod classification
//! ([`dep_types`]), package search ([`search`]), and the reverse
//! (dependents) tree used by `pnpm why` ([`dependents`]).

pub(crate) mod build;
pub(crate) mod dep_types;
pub(crate) mod dependents;
pub(crate) mod finders;
pub(crate) mod get_tree;
pub(crate) mod graph;
pub(crate) mod pkg_info;
pub(crate) mod render;
pub(crate) mod search;

use pacquet_lockfile::PkgNameVerPeer;

/// Identity of a node in the dependency graph: a workspace project
/// (importer) or an external package addressed by its depPath.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TreeNodeId {
    Importer(String),
    Package(PkgNameVerPeer),
}

impl TreeNodeId {
    /// Stable serialization used for deterministic tie-break ordering.
    /// Byte-identical to the TypeScript `serializeTreeNodeId` so the
    /// two stacks order same-name parents the same way.
    pub(crate) fn serialize(&self) -> String {
        match self {
            TreeNodeId::Importer(importer_id) => {
                format!(
                    r#"{{"type":"importer","importerId":{}}}"#,
                    serde_json::to_string(importer_id).expect("serialize importer id"),
                )
            }
            TreeNodeId::Package(dep_path) => {
                format!(
                    r#"{{"type":"package","depPath":{}}}"#,
                    serde_json::to_string(&dep_path.to_string()).expect("serialize depPath"),
                )
            }
        }
    }
}

/// One materialized node of the forward dependency tree — the shape the
/// `list` renderers consume. Counterpart of the TypeScript
/// [`DependencyNode`].
#[derive(Debug, Default, Clone)]
pub(crate) struct DependencyNode {
    pub alias: String,
    pub name: String,
    pub version: String,
    /// Absolute filesystem path of the package.
    pub path: String,
    /// Tarball URL the package was resolved from, when reconstructible.
    pub resolved: Option<String>,
    pub is_peer: bool,
    pub is_skipped: bool,
    /// `Some(true)` when the package is only reachable through
    /// `devDependencies`, `Some(false)` when only through production
    /// dependencies, `None` when reachable through both.
    pub dev: Option<bool>,
    pub optional: bool,
    pub circular: bool,
    pub deduped: bool,
    /// When `deduped`, the number of transitive dependencies elided
    /// because this subtree was already expanded elsewhere.
    pub deduped_dependencies_count: Option<u64>,
    /// Short hash distinguishing peer-dependency variants of the same
    /// `name@version`.
    pub peers_suffix_hash: Option<String>,
    pub searched: bool,
    pub search_message: Option<String>,
    pub dependencies: Vec<DependencyNode>,
}

/// Short hash of a depPath's peer-dependency suffix, used to
/// distinguish deduped instances of the same package resolved against
/// different peers. `None` when the depPath carries no peer suffix.
pub(crate) fn peers_suffix_hash(dep_path: &PkgNameVerPeer) -> Option<String> {
    let peer = dep_path.suffix.peer();
    if peer.is_empty() {
        return None;
    }
    let mut hex = pacquet_crypto_hash::create_hex_hash(peer);
    hex.truncate(4);
    Some(hex)
}

//! Lockfile-backed dependency inspection, shared by `pnpm list`, `pnpm why`,
//! `pnpm licenses`, `pnpm dedupe`, and the `@pnpm/napi` bindings.
//!
//! Rust counterpart of the TypeScript `@pnpm/deps.inspection.tree-builder`
//! and `@pnpm/deps.inspection.list` packages: a lockfile-backed dependency
//! graph ([`graph`]), a materializer that turns it into renderable
//! [`DependencyNode`] trees with deduplication and circular-reference
//! marking ([`get_tree`]), per-node metadata resolution ([`pkg_info`]),
//! dev/prod classification ([`dep_types`]), package search ([`search`]),
//! the reverse (dependents) tree behind `pnpm why` ([`dependents`]), and
//! the tree / parseable / JSON renderers for both directions ([`render`],
//! [`dependents_render`]).
//!
//! Everything here reads the lockfile and the already-materialized
//! `node_modules`; nothing resolves or fetches. `--find-by` finders, which
//! run JavaScript out of a `.pnpmfile.cjs`, stay in the CLI — this crate
//! only consumes the verdicts they record (see [`search::Searcher`]).

pub mod build;
pub mod dep_types;
pub mod dependents;
pub mod dependents_render;
pub mod get_tree;
pub mod graph;
pub mod pkg_info;
pub mod render;
pub mod search;

use pnpm_lockfile::PkgNameVerPeer;

/// Cap on every recursive walk over the dependency graph. The cycle
/// guards bound the *output*, not the recursion depth, so a hostile
/// lockfile with an absurdly long acyclic chain could otherwise
/// overflow the stack (worker threads get 2 MiB). Real dependency
/// chains stay far below this.
pub const MAX_WALK_DEPTH: usize = 256;

/// Identity of a node in the dependency graph: a workspace project
/// (importer) or an external package addressed by its depPath.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TreeNodeId {
    Importer(String),
    Package(PkgNameVerPeer),
}

impl TreeNodeId {
    /// Stable serialization used for deterministic tie-break ordering.
    /// Byte-identical to the TypeScript `serializeTreeNodeId` so the
    /// two stacks order same-name parents the same way.
    #[must_use]
    pub fn serialize(&self) -> String {
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
pub struct DependencyNode {
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
#[must_use]
pub fn peers_suffix_hash(dep_path: &PkgNameVerPeer) -> Option<String> {
    let peer = dep_path.suffix.peer();
    if peer.is_empty() {
        return None;
    }
    let mut hex = pnpm_crypto_hash::create_hex_hash(peer);
    hex.truncate(4);
    Some(hex)
}

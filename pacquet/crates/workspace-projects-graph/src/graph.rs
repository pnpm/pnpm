use indexmap::IndexMap;
use std::path::PathBuf;

/// One node of a [`ProjectGraph`]: the project itself plus the root
/// directories of the workspace siblings it depends on.
///
/// Mirrors upstream's
/// [`ProjectGraphNode`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-graph/src/index.ts#L14-L17)
/// (`{ package, dependencies }`).
#[derive(Debug, Clone)]
pub struct ProjectGraphNode<Pkg> {
    pub package: Pkg,
    /// Root directories of the workspace siblings this project depends
    /// on. The edge targets, used by the filter's dependency / dependent
    /// walks.
    pub dependencies: Vec<PathBuf>,
}

/// The workspace dependency graph, keyed by project root directory.
///
/// An [`IndexMap`] rather than a sorted map because the filter's
/// "select every project" path returns the keys in graph order, and
/// the upstream tests assert that order verbatim. Insertion order is
/// the graph's contract.
pub type ProjectGraph<Pkg> = IndexMap<PathBuf, ProjectGraphNode<Pkg>>;

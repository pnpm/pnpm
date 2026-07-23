use indexmap::IndexMap;
use std::path::PathBuf;

/// One node of a [`ProjectGraph`]: the project itself plus the root
/// directories of the workspace siblings it depends on.
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
/// "select every project" path returns the keys in graph order.
/// Insertion order is the graph's contract.
pub type ProjectGraph<Pkg> = IndexMap<PathBuf, ProjectGraphNode<Pkg>>;

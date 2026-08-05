use std::path::Path;

/// Minimal project view consumed by the graph filter: the project's
/// root directory (which doubles as the node id) and the manifest
/// `name` used for `--filter` name-pattern matching.
///
/// Narrowed to the two manifest fields the graph and the filter
/// actually read.
pub trait BaseProject {
    fn root_dir(&self) -> &Path;
    fn manifest_name(&self) -> Option<&str>;
}

/// Extends [`BaseProject`] with the manifest fields
/// [`create_projects_graph`](crate::create_projects_graph()) needs to
/// compute inter-project edges: the package `version` and its merged
/// dependency specifiers.
pub trait GraphProject: BaseProject {
    fn manifest_version(&self) -> Option<&str>;

    /// `(name, raw_specifier)` pairs merged across `peerDependencies`,
    /// `devDependencies` (unless `ignore_dev_deps`),
    /// `optionalDependencies`, and `dependencies`, in that precedence:
    /// a later group overwrites the specifier of an earlier duplicate
    /// while keeping the first-seen position.
    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)>;
}

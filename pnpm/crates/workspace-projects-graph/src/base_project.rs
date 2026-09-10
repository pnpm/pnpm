use indexmap::IndexMap;
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
/// compute inter-project edges: the package `version` and its
/// dependency specifiers.
pub trait GraphProject: BaseProject {
    fn manifest_version(&self) -> Option<&str>;

    /// `(name, raw_specifier)` pairs, one list per dependency group, in
    /// the precedence order `peerDependencies`, `devDependencies`
    /// (omitted when `ignore_dev_deps`), `optionalDependencies`,
    /// `dependencies`.
    ///
    /// The groups arrive apart so a
    /// [`DependencyRewriter`](crate::DependencyRewriter) rewrites each
    /// one as resolution rewrites the manifest's own maps: an override
    /// scoped to a range claims the declaration that carries it and not
    /// its namesake in another group.
    fn dependency_groups(&self, ignore_dev_deps: bool) -> Vec<Vec<(String, String)>>;

    /// [`Self::dependency_groups`] collapsed by
    /// [`merge_dependency_groups`].
    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        merge_dependency_groups(self.dependency_groups(ignore_dev_deps))
    }
}

/// Collapse dependency groups into the single `(name, raw_specifier)`
/// list edge resolution reads: a later group overwrites the specifier of
/// an earlier duplicate while keeping the first-seen position.
#[must_use]
pub fn merge_dependency_groups(groups: Vec<Vec<(String, String)>>) -> Vec<(String, String)> {
    let mut merged: IndexMap<String, String> = IndexMap::new();
    for group in groups {
        for (name, spec) in group {
            merged.insert(name, spec);
        }
    }
    merged.into_iter().collect()
}

use indexmap::IndexMap;
use std::path::Path;

/// Minimal project view consumed by the graph filter: the project's
/// root directory (which doubles as the node id) and the manifest
/// `name` used for `--filter` name-pattern matching.
pub trait BaseProject {
    fn root_dir(&self) -> &Path;
    fn manifest_name(&self) -> Option<&str>;
    fn merged_dependencies(&self, _ignore_dev_deps: bool) -> Vec<(String, String)> {
        Vec::new()
    }

    /// `dependencies` only.
    ///
    /// A publishable project may depend on a private workspace project
    /// from `devDependencies` and still install. The private-dependency
    /// check reads this list so those edges stay out of it.
    fn production_dependencies(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Extends [`BaseProject`] with the manifest fields
/// [`create_projects_graph`](crate::create_projects_graph()) needs to
/// compute inter-project edges: the package `version` and its
/// dependency specifiers.
pub trait GraphProject: BaseProject {
    fn manifest_version(&self) -> Option<&str>;

    /// The project's declared `(name, raw_specifier)` pairs, per group.
    /// `dev` is `None` when `ignore_dev_deps`.
    fn dependency_groups(&self, ignore_dev_deps: bool) -> DependencyGroups;
}

/// A project's declared `(name, raw_specifier)` pairs, kept per manifest
/// field so a [`DependencyRewriter`](crate::DependencyRewriter) can apply
/// rules scoped to one field.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DependencyGroups {
    pub peer: Vec<(String, String)>,
    pub dev: Option<Vec<(String, String)>>,
    pub optional: Vec<(String, String)>,
    pub prod: Vec<(String, String)>,
}

impl DependencyGroups {
    /// Collapse the groups into the single list edge resolution reads, in
    /// the precedence order `peer`, `dev`, `optional`, `prod`: a later group
    /// overwrites the specifier of an earlier duplicate while keeping the
    /// first-seen position.
    #[must_use]
    pub fn merge(self) -> Vec<(String, String)> {
        let mut merged: IndexMap<String, String> = IndexMap::new();
        let groups = [Some(self.peer), self.dev, Some(self.optional), Some(self.prod)];
        for (name, spec) in groups.into_iter().flatten().flatten() {
            merged.insert(name, spec);
        }
        merged.into_iter().collect()
    }
}

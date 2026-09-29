use crate::base_project::GraphProject;

/// Rewrites a project's declared dependency specifiers into the ones
/// the install resolves, before [`create_projects_graph`] reads them
/// for edges. `pnpm.overrides` is the source of such rewrites: an
/// override that points a dependency at a workspace sibling
/// (`workspace:`, `link:`, `file:`) turns that sibling into a
/// dependency the graph has to order, whatever range the manifest
/// declared.
///
/// [`create_projects_graph`]: crate::create_projects_graph()
pub trait DependencyRewriter: Sync {
    /// Rewrite `dependencies` — the project's merged `(name, specifier)`
    /// pairs — in place. `project` is the declaring manifest, for rules
    /// scoped to a parent (`parent>child` override keys). Dropping an
    /// entry removes the dependency from the graph.
    fn rewrite_dependencies(
        &self,
        project: &dyn GraphProject,
        dependencies: &mut Vec<(String, String)>,
    );
}

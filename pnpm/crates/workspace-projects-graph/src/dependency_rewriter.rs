use crate::base_project::{DependencyGroups, GraphProject};

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
    /// Rewrite `groups` in place. `project` is the declaring manifest, for
    /// rules scoped to a parent (`parent>child` override keys). Dropping an
    /// entry removes the dependency from the graph.
    fn rewrite_dependencies(&self, project: &dyn GraphProject, groups: &mut DependencyGroups);
}

/// The `(name, raw_specifier)` pairs [`create_projects_graph`] resolves edges
/// from: `project`'s groups rewritten by `rewriter`, then merged by
/// [`DependencyGroups::merge`].
///
/// The rewriter sees the groups before the merge, so a rule scoped to one
/// field or one declaration's range leaves that declaration's namesake in
/// another group alone, exactly as resolution does.
///
/// [`create_projects_graph`]: crate::create_projects_graph()
pub fn rewritten_dependencies<Pkg: GraphProject>(
    project: &Pkg,
    ignore_dev_deps: bool,
    rewriter: Option<&dyn DependencyRewriter>,
) -> Vec<(String, String)> {
    let Some(rewriter) = rewriter else {
        return project.merged_dependencies(ignore_dev_deps);
    };
    let mut groups = project.dependency_groups(ignore_dev_deps);
    rewriter.rewrite_dependencies(project, &mut groups);
    groups.merge()
}

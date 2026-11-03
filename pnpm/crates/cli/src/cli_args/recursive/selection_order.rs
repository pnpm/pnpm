use super::{RecursiveSelection, sequence_graph_by_project};
use pnpm_workspace::GraphPkg;
use pnpm_workspace_projects_graph::ProjectGraph;
use std::path::PathBuf;

impl<'a> RecursiveSelection<'a> {
    /// The full graph the sort resolves transitive edges through: `all` when
    /// present, otherwise `selected`. See the `all` field for why `selected`
    /// suffices when nothing narrowed the run.
    pub fn full_graph(&self) -> &ProjectGraph<GraphPkg<'a>> {
        self.all.as_ref().unwrap_or(&self.selected)
    }

    /// Sequence selected projects through the full workspace graph, using
    /// production-only edges for projects selected only by `--filter-prod`.
    pub fn sequenced_dirs(&self) -> Vec<PathBuf> {
        sequence_graph_by_project(&self.selected, |project_dir| {
            if self.prod_only_selected.contains(project_dir) {
                self.prod_all.as_ref().expect("production-only selection has a production graph")
            } else {
                self.full_graph()
            }
        })
        .order
    }
}

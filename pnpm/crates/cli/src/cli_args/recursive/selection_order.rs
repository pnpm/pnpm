use super::{RecursiveSelection, sequence_graph_by_project};
use std::path::PathBuf;

impl RecursiveSelection<'_> {
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

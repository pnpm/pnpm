//! A recursive selection detached from the projects it was made over.

use super::{
    RecursiveSelection, build_selected_graph,
    filter_graphs::{FilterGraphs, build_filter_graphs},
};
use pnpm_config::Config;
use pnpm_workspace::Project;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// The project directories a [`RecursiveSelection`] picked, owned so the
/// projects it was made over can change. [`Self::select`] rebuilds the
/// graphs from the changed projects without evaluating the selectors again,
/// so a selector that matched a project before the change keeps it.
pub struct SelectedProjectDirs {
    selected: Vec<PathBuf>,
    selected_set: HashSet<PathBuf>,
    prod_only_selected: HashSet<PathBuf>,
    narrowed: bool,
}

impl RecursiveSelection<'_> {
    pub fn selected_dirs(&self) -> SelectedProjectDirs {
        SelectedProjectDirs {
            selected: self.selected.keys().cloned().collect(),
            selected_set: self.selected.keys().cloned().collect(),
            prod_only_selected: self.prod_only_selected.clone(),
            narrowed: self.all.is_some(),
        }
    }
}

impl SelectedProjectDirs {
    pub fn contains(&self, dir: &Path) -> bool {
        self.selected_set.contains(dir)
    }

    /// The selection over `projects`, with its graph edges taken from their
    /// current manifests. `prefix` is where the command was invoked.
    pub fn select<'a>(
        &self,
        projects: &'a [Project],
        config: &Config,
        prefix: &Path,
    ) -> miette::Result<RecursiveSelection<'a>> {
        let FilterGraphs { all, prod_all } = build_filter_graphs(projects, config, prefix)?;
        if !self.narrowed {
            return Ok(RecursiveSelection {
                selected: all,
                all: None,
                prod_all: None,
                prod_only_selected: HashSet::new(),
            });
        }
        let selected =
            build_selected_graph(&all, prod_all.as_ref(), &self.selected, &self.prod_only_selected);
        Ok(RecursiveSelection {
            selected,
            all: Some(all),
            prod_all,
            prod_only_selected: self.prod_only_selected.clone(),
        })
    }
}

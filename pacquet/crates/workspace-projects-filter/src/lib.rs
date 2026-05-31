//! Pacquet port of pnpm's
//! [`@pnpm/workspace.projects-filter`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts).
//!
//! Turns `--filter` / `--filter-prod` selector strings into the set of
//! workspace projects they select. The pieces mirror upstream:
//!
//! - [`parse_project_selector()`] parses one selector string into a
//!   [`ProjectSelector`] (name glob, directory, `...`-dependents /
//!   dependencies, `^` exclude-self, `!` exclude, `[<since>]` diff).
//! - [`filter_workspace_projects`] resolves selectors against a
//!   pre-built [`ProjectGraph`].
//! - [`filter_projects`] / [`filter_projects_by_selector_objects`] build
//!   the graph (via `pacquet-workspace-projects-graph`) and run the
//!   filter, handling the `--filter-prod` production-only graph.
//!
//! Not yet ported: the `[<since>]` changed-packages selector, which
//! needs git-diff project selection. It parses, but evaluating it
//! returns [`FilterError::UnsupportedDiffSelector`].

mod filter;
mod glob;
mod parse_project_selector;
mod path_util;

pub use filter::{
    FilterError, FilterProjectsOptions, FilterWorkspaceProjectsOptions, FilteredProjects,
    WorkspaceFilter, filter_projects, filter_projects_by_selector_objects,
    filter_workspace_projects,
};
pub use parse_project_selector::{ProjectSelector, parse_project_selector};

pub use pacquet_workspace_projects_graph::{BaseProject, ProjectGraph, ProjectGraphNode};

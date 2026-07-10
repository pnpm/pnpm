//! Turns `--filter` / `--filter-prod` selector strings into the set of
//! workspace projects they select. The pieces are:
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
//! A `[<since>]` changed-packages selector selects the projects whose
//! files changed since the given git ref (`git diff --name-only`),
//! honoring `testPattern` / `changedFilesIgnorePattern`.

mod filter;
mod get_changed_projects;
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

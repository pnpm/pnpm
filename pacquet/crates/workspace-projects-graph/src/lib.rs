//! Pacquet port of pnpm's
//! [`@pnpm/workspace.projects-graph`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-graph/src/index.ts).
//!
//! Builds the directed graph of *inter-project* dependency edges for a
//! workspace: each node is a project (keyed by its root directory) and
//! its edges point at the root directories of the workspace siblings it
//! depends on. `@pnpm/workspace.projects-filter` walks this graph to
//! resolve `--filter` selectors that follow dependencies / dependents.
//!
//! The edge computation mirrors upstream's `createNode`: a dependency
//! specifier resolves to a sibling either by a local path
//! (`file:` / `link:` / a relative or absolute path) or by name plus a
//! semver / `workspace:` version match against the sibling's manifest
//! `version`. Specifiers that resolve to neither (registry tags, git
//! URLs, `npm:` aliases, ...) contribute no edge.

mod base_project;
mod create_projects_graph;
mod graph;

pub use base_project::{BaseProject, GraphProject};
pub use create_projects_graph::{
    CreateProjectsGraphOptions, CreateProjectsGraphResult, Unmatched, create_projects_graph,
};
pub use graph::{ProjectGraph, ProjectGraphNode};
pub use pacquet_fs::lexical_normalize;

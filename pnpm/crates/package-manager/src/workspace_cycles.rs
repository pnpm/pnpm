//! Report workspace projects that depend on each other in a cycle.
//!
//! The set the report covers is the set the install covers: the
//! selection a `--filter`ed or `-r` run resolved, or every project in
//! the workspace for a full install. The report comes after the
//! optimistic repeat-install short-circuit, so an install that concludes
//! "Already up to date" says nothing about cycles — pnpm returns before
//! its own check in that case.

use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_deps_restorer::{PathNode, graph_sequencer};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, ProjectGraph, create_projects_graph,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// The dependency cycles among `graph`'s projects, or `None` when they
/// can be ordered.
///
/// Edges are read from `graph` alone: an edge leaving it — a selected
/// project depending on an unselected one — is dropped rather than
/// followed, so two selected projects joined only through a third are
/// not a cycle. pnpm sequences its selected graph the same way. The
/// Self-references are reported by the sequencer but do not make a workspace
/// unorderable, so only cycles with more than one project are returned.
#[must_use]
pub fn workspace_cycles<Pkg>(graph: &ProjectGraph<Pkg>) -> Option<Vec<Vec<PathBuf>>> {
    // The sequencer runs over borrowed paths: a workspace-scale graph
    // holds tens of thousands of edges, and cloning every `PathBuf`
    // into a throwaway map cost more than the sort itself.
    let dirs: Vec<PathNode<'_>> = graph.keys().map(|dir| PathNode(dir)).collect();
    let included: HashSet<PathNode<'_>> = dirs.iter().copied().collect();
    let edges: HashMap<PathNode<'_>, Vec<PathNode<'_>>> = graph
        .iter()
        .map(|(dir, node)| {
            let dependencies = node
                .dependencies
                .iter()
                .map(|dependency| PathNode(dependency))
                .filter(|dependency| included.contains(dependency))
                .collect();
            (PathNode(dir), dependencies)
        })
        .collect();
    let cycles = graph_sequencer(&edges, &dirs)
        .cycles
        .into_iter()
        .filter(|cycle| cycle.len() > 1)
        .map(|cycle| cycle.into_iter().map(|node| node.0.to_path_buf()).collect())
        .collect::<Vec<Vec<PathBuf>>>();
    (!cycles.is_empty()).then_some(cycles)
}

/// The cycles among the projects an install covers: `selected_dirs`
/// narrows `projects` to a `--filter`ed or `-r` selection, `None` covers
/// the whole workspace.
///
/// A selected project keeps the dependency list it has in the full
/// graph; [`workspace_cycles`] then drops the edges that leave the
/// selection, which is how pnpm sequences its selected graph.
#[must_use]
pub fn install_scope_cycles(
    config: &Config,
    projects: &[Project],
    selected_dirs: Option<&HashSet<PathBuf>>,
) -> Option<Vec<Vec<PathBuf>>> {
    if projects.len() < 2 {
        return None;
    }
    let mut graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(
                config.link_workspace_packages != LinkWorkspacePackages::Off,
            ),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph;
    if let Some(selected_dirs) = selected_dirs {
        graph.retain(|dir, _| selected_dirs.contains(dir));
    }
    workspace_cycles(&graph)
}

/// Emit the cyclic-workspace-dependencies warning for `cycles`, or
/// return the error `disallowWorkspaceCycles` turns it into. `None` —
/// an orderable set — reports nothing.
///
/// Callers under `ignoreWorkspaceCycles` are expected not to have looked
/// for cycles at all; passing some anyway still reports nothing.
pub fn report_workspace_cycles<Reporter: self::Reporter>(
    config: &Config,
    workspace_dir: &Path,
    cycles: Option<&[Vec<PathBuf>]>,
) -> Result<(), CyclicWorkspaceDependenciesError> {
    let Some(cycles) = cycles.filter(|_| !config.ignore_workspace_cycles) else {
        return Ok(());
    };
    let message = format!("There are cyclic workspace dependencies{}", render_cycles(cycles));
    if config.disallow_workspace_cycles {
        return Err(CyclicWorkspaceDependenciesError { message });
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message,
        prefix: workspace_dir.to_string_lossy().into_owned(),
    }));
    Ok(())
}

/// The `: <cycle>; <cycle>` tail of the message, each cycle rendered as
/// its comma-separated project directories. Empty when the sequencer
/// could not name the cycles.
fn render_cycles(cycles: &[Vec<PathBuf>]) -> String {
    if cycles.is_empty() {
        return String::new();
    }
    let rendered = cycles
        .iter()
        .map(|cycle| cycle.iter().map(|dir| dir.to_string_lossy()).collect::<Vec<_>>().join(", "))
        .collect::<Vec<_>>()
        .join("; ");
    format!(": {rendered}")
}

#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[display("{message}")]
#[diagnostic(code(ERR_PNPM_DISALLOW_WORKSPACE_CYCLES))]
pub struct CyclicWorkspaceDependenciesError {
    #[error(not(source))]
    pub message: String,
}

#[cfg(test)]
mod tests;

//! Report workspace projects that depend on each other in a cycle.
//!
//! Two installs ask this question with different sets in hand: a
//! `--filter`ed or `-r` one reports over the selection the CLI resolved,
//! a full one over every project in the workspace, from the installer
//! where that list is already loaded. Both meet here, so the verdict and
//! the message they render cannot drift apart.

use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_deps_restorer::graph_sequencer;
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
/// cycle list can be empty for an unorderable set, which is why the
/// verdict is the `Option` rather than the list's emptiness.
#[must_use]
pub fn workspace_cycles<Pkg>(graph: &ProjectGraph<Pkg>) -> Option<Vec<Vec<PathBuf>>> {
    let dirs: Vec<PathBuf> = graph.keys().cloned().collect();
    let included: HashSet<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    let edges: HashMap<PathBuf, Vec<PathBuf>> = graph
        .iter()
        .map(|(dir, node)| {
            let dependencies = node
                .dependencies
                .iter()
                .filter(|dependency| included.contains(dependency.as_path()))
                .cloned()
                .collect();
            (dir.clone(), dependencies)
        })
        .collect();
    let sequenced = graph_sequencer(&edges, &dirs);
    (!sequenced.safe).then_some(sequenced.cycles)
}

/// The cycles among every project in the workspace — the set a full
/// install covers.
#[must_use]
pub fn workspace_wide_cycles(config: &Config, projects: &[Project]) -> Option<Vec<Vec<PathBuf>>> {
    if projects.len() < 2 {
        return None;
    }
    let graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(
                config.link_workspace_packages != LinkWorkspacePackages::Off,
            ),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph;
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

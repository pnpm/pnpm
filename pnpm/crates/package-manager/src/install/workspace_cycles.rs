//! Report workspace projects that depend on each other in a cycle.
//!
//! A `--filter`ed or `-r` command reports over its selection, from the
//! CLI layer that resolved it. This is the other half: a full install,
//! whose scope is every project in the workspace, reports here — where
//! that project list is already in hand.

use super::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_deps_restorer::graph_sequencer;
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_graph::{CreateProjectsGraphOptions, create_projects_graph};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Emit the cyclic-workspace-dependencies warning for `projects`, or
/// return the error `disallowWorkspaceCycles` turns it into.
///
/// `ignoreWorkspaceCycles` suppresses both, and with nothing to report
/// the graph is not even built.
pub(super) fn report_workspace_cycles<Reporter: self::Reporter>(
    config: &Config,
    workspace_dir: &Path,
    projects: &[Project],
) -> Result<(), CyclicWorkspaceDependenciesError> {
    if config.ignore_workspace_cycles || projects.len() < 2 {
        return Ok(());
    }
    let Some(cycles) = workspace_cycles(config, projects) else {
        return Ok(());
    };
    let message = format!("There are cyclic workspace dependencies{}", render_cycles(&cycles));
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

/// The cycles among `projects`, or `None` when they can be ordered.
///
/// The cycle list can be empty for an unorderable set, which is why the
/// verdict is the `Option` rather than the list's emptiness.
fn workspace_cycles(config: &Config, projects: &[Project]) -> Option<Vec<Vec<PathBuf>>> {
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
    let dirs: Vec<PathBuf> = graph.keys().cloned().collect();
    let edges: HashMap<PathBuf, Vec<PathBuf>> =
        graph.iter().map(|(dir, node)| (dir.clone(), node.dependencies.clone())).collect();
    let sequenced = graph_sequencer(&edges, &dirs);
    (!sequenced.safe).then_some(sequenced.cycles)
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

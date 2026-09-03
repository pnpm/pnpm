//! Topological sorting of a many-project workspace: the projects-graph
//! build plus the [`graph_sequencer()`] pass, composed the way an install
//! composes them (once for the recursive project selection, once for the
//! workspace-cycle report).
//!
//! The fixture models the workspace shape that regressed in
//! [pnpm/pnpm#14149]: thousands of projects where each depends on its
//! predecessors, so the topological order has nearly as many layers as
//! projects, plus a denser hub every hundredth project. The lockfile,
//! manifest I/O, and resolution around this phase are covered by other
//! groups; this one isolates the pre-resolution sort, which used to
//! rescan every project per layer.
//!
//! [pnpm/pnpm#14149]: <https://github.com/pnpm/pnpm/issues/14149>

use criterion::Criterion;
use pnpm_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, GraphProject, create_projects_graph,
};
use pnpm_workspace_task_scheduler::graph_sequencer;
use std::{
    collections::{HashMap, HashSet},
    hint::black_box,
    path::{Path, PathBuf},
};

const PROJECT_COUNT: usize = 4_000;
const SIBLINGS: usize = 9;
const HUB_INTERVAL: usize = 100;
const HUB_EDGES: usize = 384;

struct SyntheticProject {
    root_dir: PathBuf,
    name: String,
    dependencies: Vec<(String, String)>,
}

/// Borrowed view handed to [`create_projects_graph`], so one fixture
/// serves every iteration.
#[derive(Clone, Copy)]
struct SyntheticRef<'a>(&'a SyntheticProject);

impl BaseProject for SyntheticRef<'_> {
    fn root_dir(&self) -> &Path {
        &self.0.root_dir
    }

    fn manifest_name(&self) -> Option<&str> {
        Some(&self.0.name)
    }
}

impl GraphProject for SyntheticRef<'_> {
    fn manifest_version(&self) -> Option<&str> {
        Some("1.0.0")
    }

    fn merged_dependencies(&self, _ignore_dev_deps: bool) -> Vec<(String, String)> {
        self.0.dependencies.clone()
    }
}

fn synthetic_workspace() -> Vec<SyntheticProject> {
    (0..PROJECT_COUNT)
        .map(|index| {
            let edge_count = if index > 0 && index % HUB_INTERVAL == 0 {
                HUB_EDGES.min(index)
            } else {
                SIBLINGS
            };
            let dependencies = (index.saturating_sub(edge_count)..index)
                .map(|sibling| (format!("@synth/p{sibling:05}"), "1.0.0".to_string()))
                .collect();
            SyntheticProject {
                root_dir: PathBuf::from(format!("/workspace/packages/p{index:05}")),
                name: format!("@synth/p{index:05}"),
                dependencies,
            }
        })
        .collect()
}

pub fn bench_workspace_sort(criterion: &mut Criterion) {
    let projects = synthetic_workspace();
    let mut group = criterion.benchmark_group("workspace_sort");
    group.bench_function("chained_projects", |bencher| {
        bencher.iter(|| {
            let graph = create_projects_graph(
                projects.iter().map(SyntheticRef).collect(),
                &CreateProjectsGraphOptions {
                    link_workspace_packages: Some(true),
                    ..CreateProjectsGraphOptions::default()
                },
            )
            .graph;
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
            black_box((sequenced.cycles.len(), sequenced.order.len()))
        });
    });
    group.finish();
}

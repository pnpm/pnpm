#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "developer tool, not part of the pnpm binary"
)]

use clap::{Parser, Subcommand};
use dependencies::Reach;
use layers::Layers;
use problem::Metric;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
use tree::{GitRevision, Tree, WorkingTree};

mod cfg;
mod cycles;
mod dependencies;
mod diff;
mod graph;
mod layers;
mod lexer;
mod modules;
mod problem;
mod ratchet;
mod report;
mod tree;
mod use_tree;

#[cfg(test)]
mod tests;

const LAYERS_FILE: &str = "crate-layers.toml";

/// Check the workspace crate graph against the layers in `crate-layers.toml`.
#[derive(Debug, Parser)]
struct CliArgs {
    #[command(subcommand)]
    command: Command,

    /// The repository root. Defaults to the one this tool was built from.
    #[clap(long, default_value_os_t = default_root())]
    root: PathBuf,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fail when a crate depends on a crate in a higher layer, when a crate
    /// is in no layer, when an exception no longer matches an edge, when a
    /// crate's largest module cycle or direct dependency count differs from
    /// its `module_cycles` or `direct_dependencies` entry, or when a crate
    /// reaches more crates than its layer's `max_transitive_dependencies`.
    Check,
    /// Print, as Markdown, the crates and edges the working tree adds or
    /// removes compared to a git revision.
    Diff {
        /// The revision to compare against, such as `origin/main`.
        base: String,
    },
    /// Print the cycles among each crate's top-level modules.
    Modules {
        /// Print one crate's largest cycle and the references inside it.
        #[clap(long = "crate")]
        crate_name: Option<String>,
        /// Print a `[module_cycles]` table that accepts the current cycles.
        #[clap(long, conflicts_with = "crate_name")]
        baseline: bool,
    },
    /// Print how many workspace crates each crate depends on, directly and
    /// transitively.
    Dependencies {
        /// Print one crate's direct dependencies and the crates it reaches
        /// through them.
        #[clap(long = "crate")]
        crate_name: Option<String>,
        /// Print a `[direct_dependencies]` table that accepts the current counts.
        #[clap(long, conflicts_with = "crate_name")]
        baseline: bool,
    },
}

fn main() -> ExitCode {
    let args = CliArgs::parse();
    let result = match &args.command {
        Command::Check => check(&args.root),
        Command::Diff { base } => diff(&args.root, base),
        Command::Modules { crate_name, baseline } => {
            let view = match crate_name {
                Some(name) => View::Crate(name),
                None if *baseline => View::Baseline,
                None => View::Summary,
            };
            modules(&args.root, &view)
        }
        Command::Dependencies { crate_name, baseline } => {
            let view = match crate_name {
                Some(name) => View::Crate(name),
                None if *baseline => View::Baseline,
                None => View::Summary,
            };
            dependencies(&args.root, &view)
        }
    };
    result.unwrap_or_else(|error| {
        eprintln!("{error}");
        ExitCode::FAILURE
    })
}

fn check(root: &Path) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let layers = load_layers(&working_tree)?;
    let graph = graph::read(&working_tree)?;
    let mut problems = layers.check(&graph);
    let reports = report::read_all(&working_tree, &graph)?;
    let cycle_reports = reports
        .into_iter()
        .map(|(report, _)| report)
        .collect::<Vec<_>>();
    let cycle_sizes = cycles::sizes(&cycle_reports);
    problems.extend(ratchet::check(Metric::ModuleCycle, &cycle_sizes, &layers.module_cycles));
    let reach = Reach::new(&graph);
    problems.extend(ratchet::check(
        Metric::DirectDependencies,
        &reach.direct_counts(),
        &layers.direct_dependencies,
    ));
    problems.extend(reach.check_budgets(&layers));
    if problems.is_empty() {
        println!(
            "Every crate dependency points to its own layer or a lower one, no module cycle or \
             dependency count grew, and every crate is within its layer's transitive budget.",
        );
        return Ok(ExitCode::SUCCESS);
    }
    for problem in &problems {
        eprintln!("{problem}");
    }
    Ok(ExitCode::FAILURE)
}

fn diff(root: &Path, base: &str) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let layers = load_layers(&working_tree)?;
    let head = graph::read(&working_tree)?;
    let base_graph = graph::read(&GitRevision::open(root, base)?)?;
    print!("{}", diff::render(&base_graph, &head, &layers, base));
    Ok(ExitCode::SUCCESS)
}

/// What `crate-layers modules` and `crate-layers dependencies` print.
enum View<'name> {
    Summary,
    Baseline,
    Crate(&'name str),
}

fn modules(root: &Path, view: &View<'_>) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let reports = report::read_all(&working_tree, &graph::read(&working_tree)?)?;
    let output = match view {
        View::Crate(name) => {
            let (report, graph) = reports
                .iter()
                .find(|(report, _)| report.crate_name == *name)
                .ok_or_else(|| format!("{name} is not a workspace crate"))?;
            report::render_crate(report, graph)
        }
        View::Baseline => {
            let sizes = cycles::sizes(
                &reports
                    .into_iter()
                    .map(|(report, _)| report)
                    .collect::<Vec<_>>(),
            );
            ratchet::render_baseline(Metric::ModuleCycle, &sizes)
        }
        View::Summary => report::render_summary(&reports),
    };
    print!("{output}");
    Ok(ExitCode::SUCCESS)
}

fn dependencies(root: &Path, view: &View<'_>) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let reach = Reach::new(&graph::read(&working_tree)?);
    let output = match view {
        View::Crate(name) => reach
            .render_crate(name)
            .ok_or_else(|| format!("{name} is not a workspace crate"))?,
        View::Baseline => {
            ratchet::render_baseline(Metric::DirectDependencies, &reach.direct_counts())
        }
        View::Summary => reach.render_summary(&load_layers(&working_tree)?),
    };
    print!("{output}");
    Ok(ExitCode::SUCCESS)
}

fn load_layers(tree: &dyn Tree) -> Result<Layers, String> {
    let text = tree
        .read(LAYERS_FILE)?
        .ok_or_else(|| format!("{LAYERS_FILE} is missing"))?;
    Layers::parse(&text)
}

fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

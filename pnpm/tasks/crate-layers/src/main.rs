#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "developer tool, not part of the pnpm binary"
)]

use clap::{Parser, Subcommand};
use layers::Layers;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
use tree::{GitRevision, Tree, WorkingTree};

mod cfg;
mod cycles;
mod diff;
mod graph;
mod layers;
mod lexer;
mod modules;
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
    /// is in no layer, when an exception no longer matches an edge, or when a
    /// crate's largest module cycle differs from its `module_cycles` entry.
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
}

fn main() -> ExitCode {
    let args = CliArgs::parse();
    let result = match &args.command {
        Command::Check => check(&args.root),
        Command::Diff { base } => diff(&args.root, base),
        Command::Modules { crate_name, baseline } => {
            let view = match crate_name {
                Some(name) => ModulesView::Crate(name),
                None if *baseline => ModulesView::Baseline,
                None => ModulesView::Summary,
            };
            modules(&args.root, &view)
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
    problems.extend(cycles::check_baseline(&cycle_reports, &layers.module_cycles));
    if problems.is_empty() {
        println!(
            "Every crate dependency points to its own layer or a lower one, and no module cycle grew.",
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

/// What `crate-layers modules` prints.
enum ModulesView<'name> {
    Summary,
    Baseline,
    Crate(&'name str),
}

fn modules(root: &Path, view: &ModulesView<'_>) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let reports = report::read_all(&working_tree, &graph::read(&working_tree)?)?;
    let output = match view {
        ModulesView::Crate(name) => {
            let (report, graph) = reports
                .iter()
                .find(|(report, _)| report.crate_name == *name)
                .ok_or_else(|| format!("{name} is not a workspace crate"))?;
            report::render_crate(report, graph)
        }
        ModulesView::Baseline => report::render_baseline(&reports),
        ModulesView::Summary => report::render_summary(&reports),
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

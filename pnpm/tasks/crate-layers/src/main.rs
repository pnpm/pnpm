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

mod diff;
mod graph;
mod layers;
mod tree;

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
    /// is in no layer, or when an exception no longer matches an edge.
    Check,
    /// Print, as Markdown, the crates and edges the working tree adds or
    /// removes compared to a git revision.
    Diff {
        /// The revision to compare against, such as `origin/main`.
        base: String,
    },
}

fn main() -> ExitCode {
    let args = CliArgs::parse();
    let result = match &args.command {
        Command::Check => check(&args.root),
        Command::Diff { base } => diff(&args.root, base),
    };
    result.unwrap_or_else(|error| {
        eprintln!("{error}");
        ExitCode::FAILURE
    })
}

fn check(root: &Path) -> Result<ExitCode, String> {
    let working_tree = WorkingTree { root: root.to_path_buf() };
    let layers = load_layers(&working_tree)?;
    let problems = layers.check(&graph::read(&working_tree)?);
    if problems.is_empty() {
        println!("Every crate dependency points to its own layer or a lower one.");
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

fn load_layers(tree: &dyn Tree) -> Result<Layers, String> {
    let text = tree
        .read(LAYERS_FILE)?
        .ok_or_else(|| format!("{LAYERS_FILE} is missing"))?;
    Layers::parse(&text)
}

fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

//! The commands that read a rust-analyzer SCIP index.

use crate::{
    fields,
    graph,
    modules,
    overlaps,
    program::{
        self,
        Program,
    },
    scip,
    tree::WorkingTree,
};
use clap::Args;
use std::{
    path::PathBuf,
    process::ExitCode,
};

#[derive(Debug, Args)]
pub struct IndexArgs {
    /// The SCIP index of the working tree, which
    /// `rust-analyzer scip . --exclude-vendored-libraries --output <file>`
    /// writes when run at the repository root.
    #[clap(long)]
    index: PathBuf,
    /// The fewest fields that two functions, or two modules, have to share
    /// to be printed.
    #[clap(long, default_value_t = 4)]
    min_shared: usize,
}

#[derive(Debug, Args)]
pub struct OverlapsArgs {
    #[command(flatten)]
    index: IndexArgs,
    /// Print only the groups with a function in this crate.
    #[clap(long = "crate")]
    crate_name: Option<String>,
}

#[derive(Debug, Args)]
pub struct FieldsArgs {
    #[command(flatten)]
    index: IndexArgs,
    /// Print the functions that access the fields of this type, such as
    /// `pnpm_config::Config`, by module.
    #[clap(long = "type")]
    type_path: Option<String>,
}

pub fn overlaps(root: PathBuf, args: &OverlapsArgs) -> Result<ExitCode, String> {
    let program = read_program(root, &args.index, args.crate_name.as_deref())?;
    let mut groups = overlaps::find(&program, args.index.min_shared);
    if let Some(name) = &args.crate_name {
        let in_crate = |function: &usize| program.functions[*function].place.crate_name == *name;
        groups.retain(|group| group.functions.iter().any(in_crate));
    }
    print!("{}", overlaps::render(&program, &groups));
    Ok(ExitCode::SUCCESS)
}

pub fn fields(root: PathBuf, args: &FieldsArgs) -> Result<ExitCode, String> {
    let program = read_program(root, &args.index, None)?;
    let uses = fields::find(&program);
    let output = match &args.type_path {
        Some(type_path) => fields::render_type(&program, &uses, type_path)
            .ok_or_else(|| format!("{type_path} is not a type with fields in a workspace crate"))?,
        None => fields::render_summary(&program, &uses, args.index.min_shared),
    };
    print!("{output}");
    Ok(ExitCode::SUCCESS)
}

/// Fails before reading the index when `crate_name` is not a workspace crate.
fn read_program(
    root: PathBuf,
    args: &IndexArgs,
    crate_name: Option<&str>,
) -> Result<Program, String> {
    let working_tree = WorkingTree { root };
    let graph = graph::read(&working_tree)?;
    if let Some(name) = crate_name.filter(|name| !graph.crates.contains(*name)) {
        return Err(format!("{name} is not a workspace crate"));
    }
    let modules = modules::read_workspace(&working_tree, &graph)?;
    program::read(&scip::read(&args.index)?, &working_tree, &modules)
}

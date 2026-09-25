#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod cli_args;
mod runner;
mod workspaces;

use cli_args::CliArgs;
use runner::{Outcome, Verdict};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use which::which;

fn main() -> ExitCode {
    let args: CliArgs = clap::Parser::parse();
    let selected = match workspaces::select(&args.workspaces) {
        Ok(selected) => selected,
        Err(unknown) => {
            eprintln!("Unknown workspace {unknown:?}. Known workspaces: {}", known_names());
            return ExitCode::FAILURE;
        }
    };
    let [pnpm, cargo] = match resolve_programs(&args) {
        Ok(programs) => programs,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if !args.keep && args.work_dir.exists() {
        let work_dir = &args.work_dir;
        fs::remove_dir_all(work_dir).unwrap_or_else(|error| panic!("wipe {work_dir:?}: {error}"));
    }

    let report = selected
        .into_iter()
        .map(|workspace| {
            eprintln!("== {} ({}) ==", workspace.name, workspace.description);
            let root = args.work_dir.join(workspace.name);
            let outcome = runner::run(workspace, &root, &pnpm, &cargo);
            report_one(&outcome);
            (workspace.name, outcome)
        })
        .collect::<Vec<_>>();

    print_report(&report);
    if report.iter().all(|(_, outcome)| outcome.accepted()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The two executables, as absolute paths: every command runs in a
/// workspace directory, where a relative path would be read from.
fn resolve_programs(args: &CliArgs) -> Result<[PathBuf; 2], String> {
    let pnpm = resolve_program(&args.pnpm)?;
    let cargo = resolve_program(&args.cargo)?;
    Ok([pnpm, cargo])
}

fn report_one(outcome: &Outcome) {
    let detail = if outcome.message.is_empty() {
        String::new()
    } else {
        format!(" at {}: {}", outcome.stage, outcome.message)
    };
    eprintln!("   {} in {:.1}s{detail}", outcome.label(), outcome.duration_secs);
}

fn print_report(report: &[(&'static str, Outcome)]) {
    println!("\n=== Cargo equivalence ===");
    let width = report
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0)
        .max(9);
    for (name, outcome) in report {
        let detail = if outcome.message.is_empty() {
            String::new()
        } else {
            format!(
                "[{}] {} (kept: {})",
                outcome.stage,
                outcome.message,
                outcome.work_dir.display(),
            )
        };
        println!(
            "{:<width$}  {:<5}  {:>6.1}s  {detail}",
            name,
            outcome.label(),
            outcome.duration_secs,
        );
    }
    let known = report
        .iter()
        .filter(|(_, outcome)| outcome.verdict == Verdict::KnownDifference)
        .count();
    let unexpected = report
        .iter()
        .filter(|(_, outcome)| outcome.verdict == Verdict::Unexpected)
        .count();
    println!(
        "\n{} workspace(s), {known} known difference(s), {unexpected} unexpected",
        report.len(),
    );
}

fn resolve_program(program: &str) -> Result<PathBuf, String> {
    let path = Path::new(program);
    if path.is_file() {
        return path
            .canonicalize()
            .map_err(|error| format!("resolve {program:?}: {error}"));
    }
    which(program).map_err(|error| format!("cannot use {program:?}: {error}"))
}

fn known_names() -> String {
    workspaces::WORKSPACES
        .iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>()
        .join(", ")
}

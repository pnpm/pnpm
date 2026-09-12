#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod cli_args;
mod runner;
mod stacks;

use cli_args::{Binary, CliArgs, Layout};
use runner::{Cell, Outcome, run_cell, scaffold_template};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use which::which;

fn main() -> ExitCode {
    let args: CliArgs = clap::Parser::parse();

    let binaries = args.binary.expand();
    let layouts = args.layout.expand();
    let selected = match stacks::select(&args.stacks) {
        Ok(stacks) => stacks,
        Err(unknown) => {
            eprintln!("Unknown stack {unknown:?}. Known stacks: {}", known_stack_names());
            return ExitCode::FAILURE;
        }
    };

    ensure_program(&args.pnpm);
    if binaries.contains(&Binary::Pacquet) {
        ensure_program(&args.pacquet);
    }

    let (template_root, cells_root) = prepare_work_dir(&args);

    let mut report: Vec<(String, Outcome)> = Vec::new();
    for stack in &selected {
        report.extend(run_stack(stack, &args, &binaries, &layouts, &template_root, &cells_root));
    }

    print_report(&report);
    if report.iter().all(|(_, outcome)| outcome.passed) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The template and cell roots under a fresh work dir (kept with `--keep`).
fn prepare_work_dir(args: &CliArgs) -> (PathBuf, PathBuf) {
    let work_dir = &args.work_dir;
    if !args.keep && work_dir.exists() {
        fs::remove_dir_all(work_dir).unwrap_or_else(|error| panic!("wipe {work_dir:?}: {error}"));
    }
    let template_root = work_dir.join("templates");
    let cells_root = work_dir.join("cells");
    fs::create_dir_all(&template_root)
        .unwrap_or_else(|error| panic!("create {template_root:?}: {error}"));
    fs::create_dir_all(&cells_root)
        .unwrap_or_else(|error| panic!("create {cells_root:?}: {error}"));
    (template_root, cells_root)
}

/// Scaffold one stack's template, then run every binary × layout cell
/// against it.
fn run_stack(
    stack: &'static stacks::Stack,
    args: &CliArgs,
    binaries: &[Binary],
    layouts: &[Layout],
    template_root: &Path,
    cells_root: &Path,
) -> Vec<(String, Outcome)> {
    let scaffold_log = template_root.join(format!("{}.scaffold.log", stack.name));
    eprintln!("== scaffolding {} ({}) ==", stack.name, stack.description);
    let scaffolded = scaffold_template(&args.pnpm, stack, template_root, &scaffold_log, args.keep);
    let template_project = match scaffolded {
        Ok(path) => path,
        Err(message) => {
            eprintln!("   scaffold FAILED: {message}");
            return doomed_cells(stack, binaries, layouts, &message, &scaffold_log);
        }
    };

    let mut report = Vec::new();
    for &binary in binaries {
        for &layout in layouts {
            let cell = Cell { stack, binary, layout };
            let id = cell.id();
            eprintln!("== running {id} ==");
            let outcome = run_cell(
                &cell,
                &template_project,
                cells_root,
                &args.pnpm,
                &args.pacquet,
                !args.skip_serve,
            );
            report_cell(&outcome);
            report.push((id, outcome));
        }
    }
    report
}

/// A failed scaffold dooms every cell of this stack; they are all recorded
/// so the report stays a complete grid.
fn doomed_cells(
    stack: &'static stacks::Stack,
    binaries: &[Binary],
    layouts: &[Layout],
    message: &str,
    scaffold_log: &Path,
) -> Vec<(String, Outcome)> {
    let mut cells = Vec::new();
    for &binary in binaries {
        for &layout in layouts {
            let cell = Cell { stack, binary, layout };
            cells.push((
                cell.id(),
                Outcome {
                    passed: false,
                    duration_secs: 0.0,
                    stage: "scaffold",
                    message: message.to_string(),
                    log_path: scaffold_log.to_path_buf(),
                },
            ));
        }
    }
    cells
}

fn report_cell(outcome: &Outcome) {
    let detail = if outcome.passed {
        String::new()
    } else {
        format!(" at {}: {}", outcome.stage, outcome.message)
    };
    eprintln!(
        "   {} in {:.1}s{detail}",
        if outcome.passed { "PASS" } else { "FAIL" },
        outcome.duration_secs,
    );
}

fn print_report(report: &[(String, Outcome)]) {
    println!("\n=== Ecosystem E2E results ===");
    let id_width = report.iter().map(|(id, _)| id.len()).max().unwrap_or(0).max(4);
    for (id, outcome) in report {
        let detail = if outcome.passed {
            String::new()
        } else {
            format!("[{}] {} (log: {})", outcome.stage, outcome.message, outcome.log_path.display())
        };
        println!(
            "{:<id_width$}  {:<4}  {:>6.1}s  {detail}",
            id,
            if outcome.passed { "PASS" } else { "FAIL" },
            outcome.duration_secs,
        );
    }
    let failed = report.iter().filter(|(_, outcome)| !outcome.passed).count();
    println!("\n{} cell(s), {failed} failed", report.len());
}

fn ensure_program(program: &str) {
    if Path::new(program).is_file() {
        return;
    }
    match which(program) {
        Ok(_) => {}
        Err(which::Error::CannotFindBinaryPath) => panic!("Cannot find {program:?} in $PATH"),
        Err(error) => panic!("resolving {program:?}: {error}"),
    }
}

fn known_stack_names() -> String {
    stacks::STACKS.iter().map(|stack| stack.name).collect::<Vec<_>>().join(", ")
}

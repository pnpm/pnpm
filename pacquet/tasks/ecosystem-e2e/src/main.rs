#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod cli_args;
mod runner;
mod stacks;

use cli_args::{Binary, CliArgs};
use runner::{Cell, Outcome, run_cell, scaffold_template};
use std::{fs, path::Path, process::ExitCode};
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

    let mut report: Vec<(String, Outcome)> = Vec::new();
    for stack in &selected {
        let scaffold_log = template_root.join(format!("{}.scaffold.log", stack.name));
        eprintln!("== scaffolding {} ({}) ==", stack.name, stack.description);
        let template_project =
            match scaffold_template(&args.pnpm, stack, &template_root, &scaffold_log, args.keep) {
                Ok(path) => path,
                Err(message) => {
                    // A failed scaffold dooms every cell of this stack; record
                    // them all so the report stays a complete grid.
                    eprintln!("   scaffold FAILED: {message}");
                    for &binary in &binaries {
                        for &layout in &layouts {
                            let cell = Cell { stack, binary, layout };
                            report.push((
                                cell.id(),
                                Outcome {
                                    passed: false,
                                    duration_secs: 0.0,
                                    stage: "scaffold",
                                    message: message.clone(),
                                    log_path: scaffold_log.clone(),
                                },
                            ));
                        }
                    }
                    continue;
                }
            };

        for &binary in &binaries {
            for &layout in &layouts {
                let cell = Cell { stack, binary, layout };
                let id = cell.id();
                eprintln!("== running {id} ==");
                let outcome = run_cell(
                    &cell,
                    &template_project,
                    &cells_root,
                    &args.pnpm,
                    &args.pacquet,
                    !args.skip_serve,
                );
                eprintln!(
                    "   {} in {:.1}s{}",
                    if outcome.passed { "PASS" } else { "FAIL" },
                    outcome.duration_secs,
                    if outcome.passed {
                        String::new()
                    } else {
                        format!(" at {}: {}", outcome.stage, outcome.message)
                    },
                );
                report.push((id, outcome));
            }
        }
    }

    print_report(&report);
    if report.iter().all(|(_, outcome)| outcome.passed) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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

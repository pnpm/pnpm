//! Measures the linked size of pnpm's install engine as an embedding
//! application links it. See the README for what it reports and how.

#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "developer tool, not part of the pnpm binary"
)]

mod attribution;
mod build;
mod cli_args;
mod install_timing;
mod report;
mod sections;

use clap::Parser;
use cli_args::CliArgs;
use report::{Measurement, Report};
use std::{fs, process::ExitCode};

fn main() -> ExitCode {
    let args = CliArgs::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &CliArgs) -> Result<(), String> {
    let target_dir = args.target_dir()?;
    let target = build::host_target()?;
    let out_dir = target_dir.join("engine-size-report");
    fs::create_dir_all(&out_dir).map_err(|error| format!("create {}: {error}", out_dir.display()))?;
    let site = build::BuildSite { target_dir: &target_dir, target: &target, out_dir: &out_dir };
    let measurements = args.lto
        .iter()
        .map(|&lto| measure(args, lto, &site))
        .collect::<Result<Vec<_>, _>>()?;
    let report = Report { target, measurements };
    if let Some(path) = &args.json {
        let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
        fs::write(path, json).map_err(|error| format!("write {}: {error}", path.display()))?;
    }
    println!("{}", report.to_markdown(args.top));
    Ok(())
}

fn measure(
    args: &CliArgs,
    lto: cli_args::Lto,
    site: &build::BuildSite<'_>,
) -> Result<Measurement, String> {
    let binaries = build::build_probe(args, lto, site)?;
    let unstripped = read(&binaries.unstripped)?;
    let stripped = read(&binaries.stripped)?;
    let install = match &args.registry {
        Some(registry) => Some(install_timing::measure(&binaries.stripped, registry, args)?),
        None => None,
    };
    Ok(Measurement {
        lto,
        stripped_size: stripped.len() as u64,
        sections: sections::read_sections(&stripped)?,
        attribution: attribution::attribute(&unstripped, args.top)?,
        install,
    })
}

fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))
}

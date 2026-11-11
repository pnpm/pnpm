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
mod compare;
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
    let report = build_report(args)?;
    println!("{}", render_report(args, &report)?);
    Ok(())
}

fn build_report(args: &CliArgs) -> Result<Report, String> {
    let target_dir = args.target_dir()?;
    let target = build::host_target()?;
    let out_dir = target_dir.join("engine-size-report");
    fs::create_dir_all(&out_dir).map_err(|error| format!("create {}: {error}", out_dir.display()))?;
    let site = build::BuildSite { target_dir: &target_dir, target: &target, out_dir: &out_dir };
    let measurements = args.lto
        .iter()
        .map(|&lto| measure(args, lto, &site))
        .collect::<Result<Vec<_>, _>>()?;
    let probe_features =
        if args.report.no_default_features { "no-default-features" } else { "default" };
    Ok(Report { target, probe_features: probe_features.to_string(), measurements })
}

fn render_report(args: &CliArgs, report: &Report) -> Result<String, String> {
    let json = if args.report.json.is_some() || args.report.compare.is_some() {
        Some(serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let baseline = args.report.compare
        .as_ref()
        .map(|path| {
            let contents = read(path)?;
            std::str::from_utf8(&contents)
                .map(str::to_owned)
                .map_err(|error| format!("read {}: {error}", path.display()))
        })
        .transpose()?;
    if let (Some(path), Some(json)) = (&args.report.json, &json) {
        fs::write(path, json).map_err(|error| format!("write {}: {error}", path.display()))?;
    }
    let mut output = report.to_markdown(args.top);
    if let (Some(json), Some(baseline)) = (&json, &baseline) {
        output.push_str("\n## Change from baseline\n\n");
        output.push_str(&compare::render_comparison(json, baseline)?);
    }
    Ok(output)
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

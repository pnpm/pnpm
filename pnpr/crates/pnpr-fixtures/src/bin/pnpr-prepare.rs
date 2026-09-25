use std::path::PathBuf;

use clap::Parser;
use pnpr_fixtures::build_storage_at_with_substitutions;

/// Build verdaccio-shaped registry storage from raw package fixtures, so the
/// `pnpr` server can serve them. The pnpm test harness runs this
/// before launching the registry; pacquet's Rust tests build the same storage
/// in-process instead.
#[derive(Debug, Parser)]
#[command(name = "pnpr-prepare", version, about)]
struct Args {
    /// Directory of raw package fixtures (`<name>/<version>/...`).
    #[arg(long)]
    packages: PathBuf,

    /// Directory to write the generated storage into (cleared first).
    #[arg(long)]
    out: PathBuf,

    /// Replace an exact manifest string. May be passed more than once.
    #[arg(long = "substitute", value_parser = parse_substitution)]
    substitutions: Vec<(String, String)>,
}

fn main() {
    let args = Args::parse();
    let substitutions = args.substitutions
        .iter()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect::<Vec<_>>();
    build_storage_at_with_substitutions(&args.packages, &args.out, &substitutions);
}

fn parse_substitution(value: &str) -> Result<(String, String), String> {
    let Some((from, to)) = value.split_once('=') else {
        return Err("expected FROM=TO".to_string());
    };
    if from.is_empty() {
        return Err("FROM must not be empty".to_string());
    }
    Ok((from.to_string(), to.to_string()))
}

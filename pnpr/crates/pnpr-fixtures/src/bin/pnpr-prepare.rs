use std::path::PathBuf;

use clap::Parser;
use pnpr_fixtures::build_storage_at;

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
}

fn main() {
    let args = Args::parse();
    build_storage_at(&args.packages, &args.out);
}

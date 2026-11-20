use clap::Parser;
use std::path::PathBuf;

/// Resolve the same Cargo workspace with `cargo` and with pnpm, against the
/// live crates.io index, and compare what each one locked.
#[derive(Debug, Parser)]
pub struct CliArgs {
    /// Path to the pnpm executable, which must be the Rust CLI: Cargo
    /// support exists only in pnpm v12.
    #[clap(long, default_value = "pnpm")]
    pub pnpm: String,

    /// Path to the cargo executable resolution is compared against.
    #[clap(long, default_value = "cargo")]
    pub cargo: String,

    /// Restrict the run to workspaces whose name matches (repeatable).
    /// Defaults to every known workspace.
    #[clap(long = "workspace")]
    pub workspaces: Vec<String>,

    /// Directory holding each workspace's two resolutions. Wiped at the
    /// start of every run unless `--keep` is passed.
    #[clap(long, default_value = "cargo-equivalence-work")]
    pub work_dir: PathBuf,

    /// Keep the work directory from a previous run, leaving the lockfiles
    /// and logs of a failing comparison in place.
    #[clap(long)]
    pub keep: bool,
}

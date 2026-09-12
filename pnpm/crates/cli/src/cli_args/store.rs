//! `pacquet store` — read and act on the content-addressable store.
//!
//! `status` and `add` are the two subcommands that reach past the store
//! directory's own bookkeeping: `status` re-hashes what the store expanded
//! into the virtual store to find packages something has edited since,
//! and `add` resolves and fetches packages into the store without touching
//! any project.

mod add;
mod status;

use clap::{Args, Subcommand};
use miette::Context;
use pnpm_config::Config;
use pnpm_reporter::Reporter;
use std::path::Path;

#[derive(Debug, Subcommand)]
pub enum StoreCommand {
    /// Checks for modified packages in the store.
    /// Returns exit code 0 if the content of the package is the same as it
    /// was at the time of unpacking.
    Status,
    /// Functionally equivalent to pnpm add, except this adds new packages to the store directly
    /// without modifying any projects or files outside of the store.
    Add(StoreAddArgs),
    /// Removes unreferenced packages from the store.
    /// Unreferenced packages are packages that are not used by any projects on the system.
    /// Packages can become unreferenced after most installation operations, for instance when
    /// dependencies are made redundant.
    Prune,
    /// Returns the path to the active store directory.
    Path,
}

#[derive(Debug, Args)]
pub struct StoreAddArgs {
    /// The packages to fetch into the store, e.g. `express@4`.
    pub packages: Vec<String>,
}

impl StoreCommand {
    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        match self {
            StoreCommand::Status => status::run::<Reporter>(config, dir).await,
            StoreCommand::Add(args) => add::run::<Reporter>(config, dir, &args.packages).await,
            StoreCommand::Prune => {
                config.store_dir.prune().wrap_err("pruning store")?;
                Ok(())
            }
            StoreCommand::Path => {
                println!("{}", config.store_dir.display());
                Ok(())
            }
        }
    }
}

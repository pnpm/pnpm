use clap::Args;
use pacquet_config::{Config, check_global_bin_dir};
use std::path::Path;

use super::global::GlobalError;

/// `pacquet bin`: print the directory where pnpm installs executables.
///
/// Locally this is `<dir>/node_modules/.bin`: the `node_modules/.bin` leaf is
/// hardcoded, so a configured `modules-dir` is ignored, and the anchor is
/// `--dir` (pnpm's `config.dir`, the cwd, not the workspace root). `--global`
/// prints the resolved global bin directory instead.
#[derive(Debug, Args)]
pub struct BinArgs {
    /// Print the global executables directory
    #[clap(short = 'g', long)]
    pub global: bool,
}

impl BinArgs {
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let bin = if self.global {
            let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
            // Mirror pnpm's config reader: create then validate the global bin
            // dir for every `--global` command. `should_allow_write` is true for
            // all but `root`, so `bin` checks writability too.
            std::fs::create_dir_all(&bin).map_err(|error| {
                let bin_dir = bin.display();
                miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
            })?;
            check_global_bin_dir(&bin, std::env::var("PATH").ok().as_deref(), true)
                .map_err(miette::Report::new)?;
            bin
        } else {
            dir.join("node_modules").join(".bin")
        };
        println!("{}", bin.display());
        Ok(())
    }
}

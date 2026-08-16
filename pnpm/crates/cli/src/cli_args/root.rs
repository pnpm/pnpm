use clap::Args;
use pnpm_config::{Config, check_global_bin_dir};
use std::path::Path;

use super::global::GlobalError;

/// Print the path to the `node_modules` directory.
#[derive(Debug, Args)]
pub struct RootArgs {
    /// Print the global packages directory
    #[clap(short = 'g', long)]
    pub global: bool,
}

impl RootArgs {
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        if self.global {
            // Mirror pnpm's config reader: create then validate the global bin
            // dir for every `--global` command. `root` only prints a path, so
            // it skips the writability check (`globalDirShouldAllowWrite` is
            // false for `root` and `prefix`; see pnpm issue 2700).
            let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
            std::fs::create_dir_all(&bin).map_err(|error| {
                let bin_dir = bin.display();
                miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
            })?;
            check_global_bin_dir(&bin, std::env::var("PATH").ok().as_deref(), false)
                .map_err(miette::Report::new)?;
            let pkg_dir =
                config.global_pkg_dir.clone().ok_or(GlobalError::MissingGlobalPackageDir)?;
            println!("{}", pkg_dir.display());
        } else {
            println!("{}", dir.join("node_modules").display());
        }
        Ok(())
    }
}

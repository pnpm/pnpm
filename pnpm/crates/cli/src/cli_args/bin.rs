use super::global::GlobalError;
use clap::Args;
use pnpm_config::{Config, check_global_bin_dir};
use std::path::Path;

/// Print the directory where pnpm installs executables.
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
            // all but `root` and `prefix`, so `bin` checks writability too.
            std::fs::create_dir_all(&bin)
                .map_err(|error| {
                    let bin_dir = bin.display();
                    miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
                })?;
            check_global_bin_dir(&bin, std::env::var("PATH").ok().as_deref(), true)
                .map_err(miette::Report::new)?;
            bin
        } else {
            // Gated so an ordinary `pnpm bin` reads no manifest, and so a
            // project without one still answers.
            let project_name = config
                .applies_package_configs()
                .then(|| pnpm_workspace::read_project_name(dir, config.preferred_manifest_format))
                .flatten();
            dir.join(config.modules_dir_name_for(dir, project_name.as_deref()))
                .join(".bin")
        };
        println!("{}", bin.display());
        Ok(())
    }
}

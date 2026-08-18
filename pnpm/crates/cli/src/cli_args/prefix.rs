use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, check_global_bin_dir};
use std::path::{Path, PathBuf};

use super::global::GlobalError;

/// Print the current package prefix — the nearest directory containing a
/// `package.json`, `node_modules`, or `pnpm-workspace.yaml`.
#[derive(Debug, Args)]
pub struct PrefixArgs {
    /// Print the global prefix
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Errors specific to `pacquet prefix`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PrefixError {
    /// IO error while looking up the prefix.
    #[display("failed to access {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_CLI_PREFIX_IO_ERROR))]
    Io { path: PathBuf, source: std::io::Error },
}

/// Find the nearest directory containing package.json, `node_modules`, etc.
/// Port of findLocalPrefix from pnpm.
pub fn find_local_prefix(start_dir: &Path) -> miette::Result<PathBuf> {
    let mut name = start_dir.to_path_buf();

    while name.file_name().is_some_and(|f| f == "node_modules") {
        if let Some(parent) = name.parent() {
            name = parent.to_path_buf();
        } else {
            break;
        }
    }

    if name == start_dir { find_prefix_up(&name, &name) } else { Ok(name) }
}

fn find_prefix_up(name: &Path, original: &Path) -> miette::Result<PathBuf> {
    let mut current = name.to_path_buf();
    let targets =
        ["node_modules", "package.json", "package.json5", "package.yaml", "pnpm-workspace.yaml"];

    loop {
        for target in &targets {
            let target_path = current.join(target);
            match target_path.try_exists() {
                Ok(true) => return Ok(current),
                Ok(false) => continue,
                Err(e) => {
                    return Err(PrefixError::Io { path: target_path, source: e }.into());
                }
            }
        }

        match current.parent() {
            Some(parent) => {
                if parent == current {
                    return Ok(original.to_path_buf());
                }
                current = parent.to_path_buf();
            }
            None => return Ok(original.to_path_buf()),
        }
    }
}

impl PrefixArgs {
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        if self.global {
            // Mirror pnpm's config reader: create then validate the global bin
            // dir for every `--global` command, without the writability check
            // (`globalDirShouldAllowWrite` is false for `root` and `prefix`;
            // see pnpm issue 2700).
            let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
            std::fs::create_dir_all(&bin).map_err(|error| {
                let bin_dir = bin.display();
                miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
            })?;
            check_global_bin_dir(&bin, std::env::var("PATH").ok().as_deref(), false)
                .map_err(miette::Report::new)?;
            // pnpm's `prefix` handler prints the parent of the global packages
            // dir — the global dir root, without the layout-version leaf.
            let pkg_dir =
                config.global_pkg_dir.clone().ok_or(GlobalError::MissingGlobalPackageDir)?;
            let prefix_dir = pkg_dir.parent().ok_or(GlobalError::MissingGlobalPackageDir)?;
            println!("{}", prefix_dir.display());
            return Ok(());
        }
        let prefix_dir = find_local_prefix(dir)?;
        println!("{}", prefix_dir.display());
        Ok(())
    }
}

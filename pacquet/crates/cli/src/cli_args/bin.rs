use clap::Args;
use pacquet_config::{Config, check_global_bin_dir};
use std::path::Path;

use super::global::GlobalError;

/// `pacquet bin`: print the directory where pnpm installs executables.
///
/// Ports pnpm's `bin` handler, which prints `config.bin`
/// (<https://github.com/pnpm/pnpm/blob/fc2f33912e/pnpm11/pnpm/src/cmd/bin.ts>).
/// pnpm resolves that path in its config reader
/// (<https://github.com/pnpm/pnpm/blob/3425e8011c/pnpm11/config/reader/src/index.ts>):
/// the local value is `path.join(config.dir, 'node_modules', '.bin')` — the
/// leaf is the hardcoded `node_modules/.bin`, a configured `modules-dir` is
/// ignored, and the anchor is `config.dir` (the cwd, not the workspace root) —
/// while `--global` selects `global-bin-dir ?? <pnpm-home>/bin`, which pacquet
/// has already resolved into `config.global_bin`.
///
/// So this prints `<dir>/node_modules/.bin` from the already-canonicalized
/// `--dir` (deliberately NOT reading `config.modules_dir`, which pacquet
/// re-anchors to the workspace root inside a workspace — the same reasoning the
/// sibling `root` command documents), or the resolved global bin directory
/// under `--global`.
#[derive(Debug, Args)]
pub struct BinArgs {
    /// Print the global executables directory
    #[clap(short = 'g', long)]
    pub global: bool,
}

impl BinArgs {
    /// `--global` resolves the global executables directory
    /// (`global-bin-dir ?? <pnpm-home>/bin`), creates it, and validates it with
    /// `check_global_bin_dir` — the `PATH`-membership check plus writability
    /// (pnpm enforces the write check for every command except `root`) — before
    /// printing, mirroring pnpm's config reader, which runs the same `mkdir` +
    /// `checkGlobalBinDir` for every `--global` invocation. It errors with
    /// `ERR_PNPM_NO_GLOBAL_BIN_DIR` when no pnpm home resolves, and with the
    /// `checkGlobalBinDir` diagnostics (`ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH`,
    /// `ERR_PNPM_NO_PATH_ENV`, `ERR_PNPM_PNPM_DIR_NOT_WRITABLE`) when the
    /// directory is unusable.
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let bin = if self.global {
            let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
            // pnpm's config reader `mkdir`s the global bin dir and runs
            // `checkGlobalBinDir` for every `--global` command before the handler
            // prints; `globalDirShouldAllowWrite` is true for all but `root`, so
            // `bin` validates writability too.
            std::fs::create_dir_all(&bin).map_err(|error| {
                miette::miette!(
                    "failed to create the global bin directory {}: {error}",
                    bin.display(),
                )
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

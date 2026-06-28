use clap::Args;
use pacquet_config::Config;
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
    /// `--global` prints `config.global_bin` (`global-bin-dir ?? <pnpm-home>/bin`),
    /// erroring with `ERR_PNPM_NO_GLOBAL_BIN_DIR` when no pnpm home resolves.
    ///
    /// Divergence from pnpm worth flagging: pnpm's config reader runs
    /// `checkGlobalBinDir` (the `PATH`-membership check, plus writability for
    /// every command except `root`) and creates the directory for *every*
    /// `--global` invocation — `bin` included — before the handler prints
    /// (config/reader/src/index.ts). pacquet runs `check_global_bin_dir` only in
    /// its mutating global handlers (`add` / `remove` / `update -g`), so this
    /// read-only command neither validates `PATH` nor creates the directory,
    /// matching the sibling `outdated --global`. Porting that validation to the
    /// config layer for all read-only `-g` commands is left as a follow-up.
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let bin = if self.global {
            config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?
        } else {
            dir.join("node_modules").join(".bin")
        };
        println!("{}", bin.display());
        Ok(())
    }
}

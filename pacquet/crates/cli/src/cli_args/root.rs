use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::Path;

/// `pacquet root`: print the effective `node_modules` directory.
///
/// Ports the non-global path of pnpm's `root` handler, which is
/// `path.join(opts.dir, 'node_modules')`
/// (<https://github.com/pnpm/pnpm/blob/d3f68e2aa4/pnpm11/pnpm/src/cmd/root.ts>):
/// the leaf is the hardcoded string `node_modules` — a configured
/// `modules-dir` is ignored — and the anchor is `config.dir`, which pnpm
/// sets to the realpath of the CLI directory (the cwd, not the workspace
/// root). So this prints `<dir>/node_modules` from the already-canonicalized
/// `--dir` and deliberately does NOT read `config.modules_dir`, which pacquet
/// re-anchors to the workspace root inside a workspace.
#[derive(Debug, Args)]
pub struct RootArgs {
    /// Print the global packages directory
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Errors specific to `pacquet root`.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum RootError {
    /// `--global` is rejected because the global-dir machinery (pnpm's
    /// `@pnpm/global.commands`) is not ported to pacquet yet; refuse rather
    /// than print a wrong path.
    #[display(
        "`pacquet root --global` is not supported yet; global package management has not been ported to pacquet."
    )]
    #[diagnostic(code(pacquet_cli::root_global_unsupported))]
    GlobalUnsupported,
}

impl RootArgs {
    pub fn run(self, dir: &Path) -> miette::Result<()> {
        if self.global {
            return Err(RootError::GlobalUnsupported.into());
        }
        println!("{}", dir.join("node_modules").display());
        Ok(())
    }
}

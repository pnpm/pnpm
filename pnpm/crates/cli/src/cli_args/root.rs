use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::Path;

/// Print the path to the `node_modules` directory.
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
    /// `--global` is rejected because the global-dir machinery is not
    /// ported to pacquet yet; refuse rather than print a wrong path.
    #[display(
        "`pnpm root --global` is not supported yet; global package management has not been ported to pnpm."
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

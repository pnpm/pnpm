use clap::Subcommand;
use miette::Context;
use pacquet_config::Config;

#[derive(Debug, Subcommand)]
pub enum StoreCommand {
    /// Checks for modified packages in the store.
    Store,
    /// Functionally equivalent to pnpm add, except this adds new packages to the store directly
    /// without modifying any projects or files outside of the store.
    Add,
    /// Removes unreferenced packages from the store.
    /// Unreferenced packages are packages that are not used by any projects on the system.
    /// Packages can become unreferenced after most installation operations, for instance when
    /// dependencies are made redundant.
    Prune,
    /// Returns the path to the active store directory.
    Path,
}

impl StoreCommand {
    /// Execute the subcommand. `config` is fallible so a malformed
    /// `pnpm-workspace.yaml` surfaces as a hard error here too.
    pub fn run<'a>(
        self,
        config: impl FnOnce() -> miette::Result<&'a Config>,
    ) -> miette::Result<()> {
        match self {
            StoreCommand::Store => {
                panic!("Not implemented")
            }
            StoreCommand::Add => {
                panic!("Not implemented")
            }
            StoreCommand::Prune => {
                config()?.store_dir.prune().wrap_err("pruning store")?;
            }
            StoreCommand::Path => {
                println!("{}", config()?.store_dir.display());
            }
        }

        Ok(())
    }
}

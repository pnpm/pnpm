use crate::cli_args::star::star_action;
use clap::Parser;
use pnpm_config::Config;

#[derive(Debug, Parser)]
pub struct UnstarArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    pub package_name: String,
}

impl UnstarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        star_action(config, self.registry.as_deref(), &self.package_name, false).await
    }
}

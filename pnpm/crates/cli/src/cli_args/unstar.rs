use crate::cli_args::star::star_action;
use clap::Parser;
use pacquet_config::Config;

#[derive(Debug, Parser)]
pub struct UnstarArgs {
    pub package_name: String,
}

impl UnstarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        star_action(config, &self.package_name, false).await
    }
}

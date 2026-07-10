use crate::cli_args::registry_client::build_registry_client;
use clap::Parser;
use pacquet_config::Config;
use pacquet_network::RetryOpts;
use std::time::Duration;

use crate::cli_args::star::{StarError, fetch_star};

#[derive(Debug, Parser)]
pub struct UnstarArgs {
    pub package_name: String,
}

impl UnstarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        let auth_header =
            config.auth_headers.for_url(&config.registry).ok_or(StarError::Unauthorized)?;
        let http_client = build_registry_client(config)?;
        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };
        fetch_star(
            &config.registry,
            &http_client,
            &auth_header,
            retry_opts,
            &self.package_name,
            false,
        )
        .await
    }
}

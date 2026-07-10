use crate::cli_args::registry_client::build_registry_client;
use clap::Parser;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_network::RetryOpts;
use std::time::Duration;

use crate::cli_args::star::{StarError, fetch_star};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UnstarError {
    #[display("You must be logged in to unstar packages")]
    #[diagnostic(code(ERR_PNPM_STAR_UNAUTHORIZED))]
    Unauthorized,

    #[display("Failed to unstar package: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    Failed { status: u16, status_text: String },
}

#[derive(Debug, Parser)]
pub struct UnstarArgs {
    pub package_name: String,
}

impl UnstarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        let auth_header =
            config.auth_headers.for_url(&config.registry).ok_or(UnstarError::Unauthorized)?;
        let http_client = build_registry_client(config)?;
        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };
        match fetch_star(
            &config.registry,
            &http_client,
            &auth_header,
            retry_opts,
            &self.package_name,
            false,
        )
        .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                if let Some(star_err) = err.downcast_ref::<StarError>() {
                    match star_err {
                        StarError::Failed { status, status_text } => {
                            Err(UnstarError::Failed { status: *status, status_text: status_text.clone() }.into())
                        }
                        StarError::Unauthorized => Err(UnstarError::Unauthorized.into()),
                    }
                } else {
                    Err(err)
                }
            }
        }
    }
}

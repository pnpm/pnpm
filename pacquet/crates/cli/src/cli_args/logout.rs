//! `pacquet logout` — revoke the registry auth token and remove it from
//! `auth.ini`. The command logic lives in `pacquet-auth-commands`; this
//! module is the thin CLI adapter that resolves config into
//! [`LogoutOptions`].

use std::{collections::HashMap, time::Duration};

use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pacquet_auth_commands::logout::{Host as AuthHost, LogoutOptions, logout};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_reporter::Reporter;

/// Log out of an npm registry.
#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// The registry to log out of.
    #[clap(long)]
    pub registry: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LogoutCliError {
    /// pacquet-specific guard: pnpm always resolves a `configDir`, but
    /// pacquet leaves [`Config::config_dir`] `None` when no home directory
    /// can be located, and `logout` cannot find `auth.ini` without it.
    #[display("Could not determine the pnpm config directory to locate auth.ini")]
    #[diagnostic(code(ERR_PNPM_NO_CONFIG_DIR))]
    NoConfigDir,
}

impl LogoutArgs {
    pub async fn run<Reporter: self::Reporter>(
        self,
        config: &Config,
        prefix: &str,
    ) -> miette::Result<()> {
        let Some(config_dir) = config.config_dir.as_deref() else {
            return Err(LogoutCliError::NoConfigDir.into());
        };

        let http_client = ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .into_diagnostic()?;

        // Reconstruct the subset of pnpm's `config.authConfig` the command
        // reads: `<nerf-darted-uri>:_authToken` -> raw token.
        let auth_config: HashMap<String, String> = config
            .auth_tokens_by_uri
            .iter()
            .map(|(uri, token)| (format!("{uri}:_authToken"), token.clone()))
            .collect();

        let retry = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };

        let message = logout::<AuthHost, Reporter>(
            &http_client,
            LogoutOptions {
                // `--registry` wins; otherwise the resolved registry,
                // which already folds in `.npmrc` and the npmjs default.
                registry: self.registry.as_deref().or(Some(config.registry.as_str())),
                auth_config: &auth_config,
                config_dir,
                retry,
                prefix,
            },
        )
        .await?;

        println!("{message}");
        Ok(())
    }
}

#[cfg(test)]
mod tests;

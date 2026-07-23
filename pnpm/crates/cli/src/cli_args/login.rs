//! `pacquet login` / `pacquet adduser` — authenticate with an npm registry
//! and record the token in `auth.ini`. The command logic lives in
//! `pacquet-auth-commands`; this module is the thin CLI adapter that resolves
//! config into [`LoginOptions`].

use std::{path::Path, time::Duration};

use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pacquet_auth_commands::login::{Host as AuthHost, LoginHost, LoginOptions, login};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, ThrottledClient};
use pacquet_reporter::Reporter;

/// Log in to an npm registry.
#[derive(Debug, Args)]
pub struct LoginArgs {
    /// The registry to log in to.
    #[clap(long)]
    pub registry: Option<String>,

    /// Associate the login token with a package scope and record the
    /// scope-to-registry mapping.
    #[clap(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoginCliError {
    /// pacquet-specific guard: pnpm always resolves a `configDir`, but
    /// pacquet leaves [`Config::config_dir`] `None` when no home directory
    /// can be located, and `login` cannot write `auth.ini` without it.
    #[display("Could not determine the pnpm config directory to locate auth.ini")]
    #[diagnostic(code(ERR_PNPM_NO_CONFIG_DIR))]
    NoConfigDir,
}

impl LoginArgs {
    pub async fn run<Reporter: self::Reporter>(self, config: &Config) -> miette::Result<()> {
        let message = self.execute::<AuthHost, Reporter>(config).await?;
        println!("{message}");
        Ok(())
    }

    /// The testable core of [`run`](Self::run): guard the config directory,
    /// build the registry HTTP client from `config`, and perform the login,
    /// returning the success message. Generic over the capability host `Sys` so
    /// a test can drive it with a fake host over a mock registry and assert on
    /// the returned message; [`run`](Self::run) binds the production
    /// [`AuthHost`] and writes that message to stdout.
    async fn execute<Sys, Reporter>(&self, config: &Config) -> miette::Result<String>
    where
        Sys: LoginHost,
        Reporter: self::Reporter,
    {
        let Some(config_dir) = config.config_dir.as_deref() else {
            return Err(LoginCliError::NoConfigDir.into());
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

        let message =
            login::<Sys, Reporter>(&http_client, self.login_options(config, config_dir)).await?;
        Ok(message)
    }

    /// Resolve the `--registry` / `--scope` flags and `config` into
    /// [`LoginOptions`]. Split out from [`run`](Self::run) so the flag-vs-config
    /// resolution is unit-testable without a network client.
    fn login_options<'a>(&'a self, config: &'a Config, config_dir: &'a Path) -> LoginOptions<'a> {
        LoginOptions {
            // `--registry` wins; otherwise the resolved registry, which already
            // folds in `.npmrc` and the npmjs default.
            registry: self.registry.as_deref().or(Some(config.registry.as_str())),
            // `--scope` wins; otherwise the resolved `scope`, which folds in the
            // `.npmrc` / `pnpm-workspace.yaml` `scope` option.
            scope: self.scope.as_deref().or(config.scope.as_deref()),
            config_dir,
            fetch_retries: config.fetch_retries,
            fetch_retry_factor: config.fetch_retry_factor,
            fetch_retry_mintimeout: config.fetch_retry_mintimeout,
            fetch_retry_maxtimeout: config.fetch_retry_maxtimeout,
            fetch_timeout: config.fetch_timeout,
        }
    }
}

#[cfg(test)]
mod tests;

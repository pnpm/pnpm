use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{NetworkSettings, ThrottledClient};
use std::time::Duration;

/// The npm CLI's default `fetch-timeout`. The registry can take longer than
/// pnpm's default `fetchTimeout` to answer a publish request, and a publish
/// request re-sent after a timeout fails with 409 Conflict ("Failed to save
/// packument") while the first one is still being processed
/// (<https://github.com/pnpm/pnpm/issues/11454>).
const MIN_PUBLISH_FETCH_TIMEOUT: Duration = Duration::from_mins(5);

/// Build the network client a one-off registry query (`whoami`, `ping`, ...)
/// makes its request through, from the same proxy / TLS / timeout config as
/// the install client ([`crate::state::State::init`]).
pub fn build_registry_client(config: &Config) -> miette::Result<ThrottledClient> {
    build_client(config, &config.network_settings())
}

/// Build the network client `pnpm publish` sends its requests through. It is
/// the registry client with a `fetchTimeout` of at least
/// [`MIN_PUBLISH_FETCH_TIMEOUT`].
pub fn build_publish_client(config: &Config) -> miette::Result<ThrottledClient> {
    build_client(config, &publish_network_settings(config))
}

fn build_client(config: &Config, settings: &NetworkSettings) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(&config.proxy, &config.tls, &config.tls_by_uri, settings)
        .into_diagnostic()
        .wrap_err("create the network client for the registry request")
}

fn publish_network_settings(config: &Config) -> NetworkSettings {
    let mut settings = config.network_settings();
    settings.fetch_timeout = settings.fetch_timeout.max(MIN_PUBLISH_FETCH_TIMEOUT);
    settings
}

#[cfg(test)]
mod tests;

use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, ThrottledClient};
use std::time::Duration;

/// Build the network client a one-off registry query (`whoami`, `ping`, ...)
/// makes its request through, from the same proxy / TLS / timeout config as
/// the install client ([`crate::state::State::init`]).
pub fn build_registry_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for the registry request")
}

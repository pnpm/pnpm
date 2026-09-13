use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;

/// Build the network client a one-off registry query (`whoami`, `ping`, ...)
/// makes its request through, from the same proxy / TLS / timeout config as
/// the install client ([`crate::state::State::init`]).
pub fn build_registry_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err("create the network client for the registry request")
}

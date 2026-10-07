use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    NetworkSettings, RedirectGuard, ThrottledClient, normalize_registry_url, origins_redirect_guard,
};
use std::{collections::HashMap, time::Duration};

/// The npm CLI's default `fetch-timeout`. The registry can take longer than
/// pnpm's default `fetchTimeout` to answer a publish request, and a publish
/// request re-sent after a timeout fails with 409 Conflict ("Failed to save
/// packument") while the first one is still being processed
/// (<https://github.com/pnpm/pnpm/issues/11454>).
const MIN_PUBLISH_FETCH_TIMEOUT: Duration = Duration::from_mins(5);

/// Resolve the configured registries map, overriding the `"default"` entry
/// when a CLI registry override is provided.
pub fn resolve_registries_with_override(
    config: &Config,
    registry_override: Option<&str>,
) -> HashMap<String, String> {
    let mut registries: HashMap<String, String> = config
        .resolved_registries()
        .into_iter()
        .collect();
    if let Some(registry) = registry_override {
        registries.insert("default".to_string(), normalize_registry_url(registry).into_owned());
    }
    registries
}

/// Build the network client a one-off registry query makes its request through,
/// optionally restricted by a redirect guard.
pub fn build_registry_client_with_guard(
    config: &Config,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs_with_guard(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
        redirect_guard,
    )
    .into_diagnostic()
    .wrap_err("create the network client for the registry request")
}

/// Build a registry client that restricts redirects to the allowed registries
/// when an OTP is provided. If no OTP is provided, the client is unguarded.
pub fn build_registry_client_with_otp_guard<'a, Registries>(
    config: &Config,
    otp: Option<&str>,
    allowed_registries: Registries,
) -> miette::Result<ThrottledClient>
where
    Registries: IntoIterator<Item = &'a str>,
{
    let guard = otp.map(|_| origins_redirect_guard(allowed_registries));
    build_registry_client_with_guard(config, guard.as_ref())
}

/// Build the network client a one-off registry query (`whoami`, `ping`, ...)
/// makes its request through, from the same proxy / TLS / timeout config as
/// the install client ([`crate::state::State::init`]).
pub fn build_registry_client(config: &Config) -> miette::Result<ThrottledClient> {
    build_registry_client_with_guard(config, None)
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

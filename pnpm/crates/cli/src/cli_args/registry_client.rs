use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    NetworkSettings, RedirectGuard, ThrottledClient, escaped_package_name, normalize_registry_url,
    origins_redirect_guard,
};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
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

/// Pick and normalize the registry URL for a given package name.
pub fn resolve_registry_for_package(
    registries: &HashMap<String, String>,
    package_name: &str,
    publish_registry: Option<&str>,
) -> String {
    let raw = pick_registry_for_package(registries, package_name, publish_registry);
    normalize_registry_url(&raw).into_owned()
}

/// Look up the authorization header for the given package on the registry URL.
pub fn auth_header_for_package(
    config: &Config,
    registry_url: &str,
    package_name: &str,
) -> Option<String> {
    config.auth_headers.for_url_with_package(registry_url, Some(package_name))
}

/// Join a relative path against a normalized registry URL, preserving any
/// path prefix in the registry URL.
pub fn join_registry_endpoint(registry_url: &str, path: &str) -> Result<String, url::ParseError> {
    reqwest::Url::parse(&normalize_registry_url(registry_url))
        .and_then(|url| url.join(path))
        .map(|url| url.to_string())
}

/// Join a package name onto a registry URL using the registry's package-escaping convention.
pub fn package_endpoint_url(
    registry_url: &str,
    package_name: &str,
) -> Result<String, url::ParseError> {
    join_registry_endpoint(registry_url, &escaped_package_name(package_name))
}

/// Attach authorization and one-time password headers to a request builder when present.
pub fn apply_auth_and_otp(
    mut builder: reqwest::RequestBuilder,
    auth_header: Option<&str>,
    otp: Option<&str>,
) -> reqwest::RequestBuilder {
    if let Some(auth) = auth_header {
        builder = builder.header("authorization", auth);
    }
    if let Some(otp) = otp {
        builder = builder.header("npm-otp", otp);
    }
    builder
}

/// Build the network client a one-off registry query makes its request through,
/// optionally restricted by a redirect guard.
pub fn build_registry_client_with_guard(
    config: &Config,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    build_client_with_settings_and_guard(config, &config.network_settings(), redirect_guard)
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
    build_client_with_settings_and_guard(config, &publish_network_settings(config), None)
}

fn build_client_with_settings_and_guard(
    config: &Config,
    settings: &NetworkSettings,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs_with_guard(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        settings,
        redirect_guard,
    )
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

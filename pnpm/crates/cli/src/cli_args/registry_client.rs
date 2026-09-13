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

/// Restrict redirects to configured registry origins. Custom authentication
/// headers such as `npm-otp` must not reach an unrelated redirect target.
pub(super) fn registry_redirect_guard<'a>(
    registries: impl Iterator<Item = &'a str>,
) -> pnpm_network::RedirectGuard {
    let origins: Vec<(String, String, Option<u16>)> = registries
        .filter_map(|registry| {
            let url = reqwest::Url::parse(registry).ok()?;
            Some((
                url.scheme().to_string(),
                url.host_str()?.to_string(),
                url.port(),
            ))
        })
        .collect();
    let guard: pnpm_network::RedirectGuard =
        std::sync::Arc::new(move |target: &reqwest::Url| -> bool {
            origins
                .iter()
                .any(|(scheme, host, port)| {
                    target.scheme() == scheme
                        && target.host_str() == Some(host.as_str())
                        && target.port() == *port
                })
        });
    guard
}

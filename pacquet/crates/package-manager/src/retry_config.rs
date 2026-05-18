use pacquet_config::Config;
use pacquet_tarball::RetryOpts;
use std::time::Duration;

/// Build the [`RetryOpts`] the tarball download path expects from the
/// resolved [`Config`] config. Centralised so the two `install_package_*`
/// call sites can't drift over time.
pub(crate) fn retry_opts_from_config(config: &Config) -> RetryOpts {
    RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    }
}

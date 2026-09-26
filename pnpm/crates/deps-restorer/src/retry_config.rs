use pnpm_config::Config;
use pnpm_tarball::RetryOpts;

/// Build the [`RetryOpts`] the tarball download path expects from the
/// resolved [`Config`] config. Centralised so the two `install_package_*`
/// call sites can't drift over time.
pub fn retry_opts_from_config(config: &Config) -> RetryOpts {
    config.retry_opts()
}

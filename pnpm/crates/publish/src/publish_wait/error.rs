use std::time::Duration;

use pnpm_diagnostics::miette::{self, Diagnostic};

use crate::registry_config_keys::NormalizedRegistryUrl;

#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishWaitError {
    #[display(
        "Timed out after {milliseconds}ms waiting for {packages} to become available from {registry}"
    )]
    #[diagnostic(
        code(ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT),
        help(
            "The upload was accepted or the version was already published. It may become available later. Do not publish the same version again."
        )
    )]
    Timeout { packages: String, registry: String, milliseconds: u128 },
    #[display("Could not confirm availability of {package}: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_PUBLISH_AVAILABILITY_CHECK_FAILED),
        help(
            "The upload was accepted or the version was already published. Correct the registry access or response before checking again."
        )
    )]
    Check { package: String, reason: String },
}

impl PublishWaitError {
    pub(crate) fn timeout(
        packages: impl Iterator<Item = String>,
        registry: &NormalizedRegistryUrl,
        timeout: Duration,
    ) -> Self {
        Self::Timeout {
            packages: packages.collect::<Vec<_>>().join(", "),
            registry: pnpm_network::redact_url_for_display(registry.as_str()),
            milliseconds: timeout.as_millis(),
        }
    }

    pub(crate) fn check(name: &str, version: &str, reason: String) -> Self {
        Self::Check { package: format!("{name}@{version}"), reason }
    }
}

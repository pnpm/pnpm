use pnpr_error::RegistryError;
use serde::Deserialize;
use std::{fmt, net::SocketAddr};

/// The YAML `metrics:` block. pnpr serves Prometheus metrics only when it is
/// present, and only to a scraper that holds `token` or reaches `listen`.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricsConfig {
    /// The bearer secret a scraper sends.
    #[serde(default)]
    pub token: Option<String>,
    /// The address of a listener that serves only metrics. When set, the main
    /// listener does not serve them.
    #[serde(default)]
    pub listen: Option<SocketAddr>,
}

impl MetricsConfig {
    /// The shortest `token` pnpr accepts, not counting surrounding whitespace.
    pub const MIN_TOKEN_LEN: usize = 32;

    pub(super) fn validate(&self, server_listen: SocketAddr) -> Result<(), RegistryError> {
        match (&self.token, self.listen) {
            (None, None) => Err(RegistryError::InvalidConfig {
                reason: "metrics needs a token, a listen address, or both".to_string(),
            }),
            (_, Some(listen)) if listen == server_listen => Err(RegistryError::InvalidConfig {
                reason: format!("metrics.listen {listen} is the server's own listen address"),
            }),
            (Some(token), _) if token.trim().len() < Self::MIN_TOKEN_LEN => {
                Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "metrics.token must be at least {} characters long",
                        Self::MIN_TOKEN_LEN,
                    ),
                })
            }
            _ => Ok(()),
        }
    }
}

impl fmt::Debug for MetricsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetricsConfig")
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("listen", &self.listen)
            .finish()
    }
}

//! Port of `publish/oidc`: OpenID-Connect trusted publishing — fetch a CI
//! id-token, exchange it for a registry auth token, and decide whether to
//! attach provenance.
//!
//! Each step is generic over a single `Sys` type parameter carrying only the
//! capabilities it consumes ([`EnvVar`](crate::EnvVar),
//! [`CiInfo`](crate::CiInfo), [`Clock`](crate::Clock),
//! [`OidcFetch`](crate::OidcFetch)), so a test drives the external-service
//! happy path with `fn`-bound unit-struct fakes instead of a live registry.

mod auth_token;
mod id_token;
mod provenance;

pub use auth_token::{AuthTokenError, fetch_auth_token};
pub use id_token::{GetIdTokenError, IdTokenError, get_id_token};
pub use provenance::{DetermineProvenanceError, ProvenanceError, determine_provenance};

/// npm-package-arg's `escapedName`: percent-encode the scope separator so the
/// package name is a single URL path segment (`@scope/name` → `@scope%2fname`).
pub(crate) fn escaped_package_name(name: &str) -> String {
    name.replace('/', "%2f")
}

/// The fetch-retry / timeout knobs the OIDC requests forward, sourced from the
/// publish options. Ports the `Pick<PublishPackedPkgOptions, 'fetchRetries' |
/// ...>` shared by all three OIDC steps.
#[derive(Debug, Default, Clone)]
pub struct OidcHttpOptions {
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<f64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_timeout: Option<u64>,
}

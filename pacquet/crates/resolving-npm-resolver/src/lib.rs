//! Pacquet port of the verifier surface of pnpm's
//! [`@pnpm/resolving.npm-resolver`](https://github.com/pnpm/pnpm/tree/2a9bd897bf/resolving/npm-resolver/src/).
//!
//! Today this crate ports the [`createNpmResolutionVerifier`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts)
//! pipeline: a [`pacquet_resolving_resolver_base::ResolutionVerifier`]
//! that re-applies `minimumReleaseAge` and `trustPolicy='no-downgrade'`
//! to every npm-resolved lockfile entry the install loads.
//!
//! The full upstream package also exposes resolution helpers
//! (`pickPackage`, `parseBareSpecifier`, …) that pacquet doesn't have
//! a use for yet — those land alongside a real resolver when one
//! arrives.

mod create_npm_resolution_verifier;
mod errors;
mod fetch_attestation_published_at;
mod fetch_full_metadata;
mod fetch_full_metadata_cached;
mod lookup_context;
mod mirror;
mod named_registry;
mod registry_url;
mod trust_checks;
mod violation_codes;

pub use create_npm_resolution_verifier::{
    CreateNpmResolutionVerifierOptions, NpmResolutionVerifier, create_npm_resolution_verifier,
};
pub use errors::FetchMetadataError;
pub use fetch_attestation_published_at::{FetchAttestationOptions, fetch_attestation_published_at};
pub use fetch_full_metadata::{FetchFullMetadataOptions, fetch_full_metadata};
pub use fetch_full_metadata_cached::{FetchFullMetadataCachedOptions, fetch_full_metadata_cached};
pub use mirror::{ABBREVIATED_META_DIR, FULL_META_DIR};
pub use named_registry::{
    BUILTIN_NAMED_REGISTRIES, build_named_registry_prefixes, pick_registry_for_package,
    pick_registry_for_version,
};
pub use trust_checks::{
    TrustCheckOptions, TrustEvidence, TrustViolation, fail_if_trust_downgraded, get_trust_evidence,
};
pub use violation_codes::{MINIMUM_RELEASE_AGE_VIOLATION_CODE, TRUST_DOWNGRADE_VIOLATION_CODE};

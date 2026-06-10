//! Pacquet port of pnpm's
//! [`@pnpm/resolving.npm-resolver`](https://github.com/pnpm/pnpm/tree/f657b5cb44/resolving/npm-resolver/src/).
//!
//! Two surfaces:
//!
//! - **Resolver.** [`NpmResolver`] implements the
//!   [`Resolver`](pacquet_resolving_resolver_base::Resolver) trait.
//!   Ports upstream's
//!   [`createNpmResolver` → `resolveNpm`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L192-L611):
//!   takes a [`WantedDependency`](pacquet_resolving_resolver_base::WantedDependency),
//!   runs [`parse_bare_specifier()`], picks a version through
//!   [`pick_package()`], and returns the
//!   [`ResolveResult`](pacquet_resolving_resolver_base::ResolveResult)
//!   the install layer consumes.
//! - **Verifier.** [`create_npm_resolution_verifier()`] is the
//!   [`ResolutionVerifier`](pacquet_resolving_resolver_base::ResolutionVerifier)
//!   the lockfile-verification gate uses. Re-applies
//!   `minimumReleaseAge` and `trustPolicy='no-downgrade'` to every
//!   npm-resolved lockfile entry the install loads.

mod create_npm_resolution_verifier;
mod errors;
mod fetch_attestation_published_at;
mod fetch_full_metadata;
mod fetch_full_metadata_cached;
mod lookup_context;
mod mirror;
mod named_registry;
mod named_registry_resolver;
mod npm_resolver;
mod parse_bare_specifier;
mod pick_package;
mod pick_package_from_meta;
mod registry_url;
mod resolve_from_workspace;
mod trust_checks;
mod violation_codes;
mod workspace_pref_to_npm;

pub use create_npm_resolution_verifier::{
    CreateNpmResolutionVerifierOptions, DistStats, NpmResolutionVerifier, ObservedDistStats,
    create_npm_resolution_verifier, observed_dist_stats_sink,
};
pub use errors::FetchMetadataError;
pub use fetch_attestation_published_at::{FetchAttestationOptions, fetch_attestation_published_at};
pub use fetch_full_metadata::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
};
pub use fetch_full_metadata_cached::{FetchFullMetadataCachedOptions, fetch_full_metadata_cached};
pub use mirror::{ABBREVIATED_META_DIR, FULL_META_DIR};
pub use named_registry::{
    BUILTIN_NAMED_REGISTRIES, MergeNamedRegistriesError, build_named_registry_prefixes,
    merge_named_registries, pick_registry_for_package, pick_registry_for_version,
};
pub use named_registry_resolver::NamedRegistryResolver;
pub use npm_resolver::NpmResolver;
pub use parse_bare_specifier::{
    JsrRegistryPackageSpec, NamedRegistryPackageSpec, ParseNamedRegistrySpecifierError,
    parse_bare_specifier, parse_jsr_specifier_to_registry_package_spec,
    parse_named_registry_specifier_to_registry_package_spec,
};
pub use pick_package::{
    InMemoryPackageMetaCache, MirrorPersistError, PackageMetaCache, PackumentFetchLocker,
    PickPackageContext, PickPackageError, PickPackageOptions, PickPackageResult,
    PickedManifestCache, persist_meta_to_mirror, pick_package, shared_in_memory_cache,
    shared_packument_fetch_locker, shared_picked_manifest_cache,
};
pub use pick_package_from_meta::{
    PickPackageFromMetaError, PickPackageFromMetaOptions, PickVersionByVersionRangeOptions,
    RegistryPackageSpec, RegistryPackageSpecType, filter_pkg_metadata_by_publish_date,
    pick_lowest_version_by_version_range, pick_package_from_meta, pick_version_by_version_range,
};
pub use registry_url::to_registry_url;
pub use resolve_from_workspace::{
    ResolveFromWorkspaceError, ResolveFromWorkspaceOptions, try_resolve_from_workspace,
};
pub use trust_checks::{
    TrustCheckOptions, TrustEvidence, TrustViolation, fail_if_trust_downgraded, get_trust_evidence,
};
pub use violation_codes::{MINIMUM_RELEASE_AGE_VIOLATION_CODE, TRUST_DOWNGRADE_VIOLATION_CODE};
pub use workspace_pref_to_npm::{InvalidWorkspaceSpecError, workspace_pref_to_npm};

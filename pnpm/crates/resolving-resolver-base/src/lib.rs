//! The resolver-base seam. Two seams live here:
//!
//! 1. **Verifier seam** — [`ResolutionVerifier`] and friends, used by
//!    every resolver-side policy check. Pacquet's
//!    lockfile-verification runner depends on the trait without pulling
//!    in any specific resolver.
//!
//! 2. **Dispatcher seam** — [`WantedDependency`], [`ResolveOptions`],
//!    [`ResolveResult`], the [`Resolver`] trait, and the latest-version
//!    companion. Per-protocol resolvers implement
//!    [`Resolver`]; the default-resolver dispatcher composes them into
//!    the chain.
//!
//! Both seams sit in the same crate because several types cross over
//! (a verifier needs [`pnpm_lockfile::LockfileResolution`]; a
//! resolver result *also* carries one).
//!
//! The crate also owns the resolution settings both seams read, such as
//! [`TrustPolicy`] and [`NetworkPolicy`]. The configuration loads them and
//! re-exports them.

pub use errors::{
    GitResolveError, NoMatchingVersionError, RegistryResponseError, RegistryResponseErrorOptions,
};
pub use network_policy::NetworkPolicy;
pub use peer_range::{get_peer_version_range, is_acceptable_peer_spec, is_valid_peer_range};
pub use pnpm_semver_include_prerelease::{
    ANY_VERSION_RANGE, is_any_version_range, is_valid_semver_range,
};
pub use policy::{LinkWorkspacePackages, SaveWorkspaceProtocol, TrustPolicy};
pub use preferred_versions::{
    DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, PreferredVersions,
    PreferredVersionsOverlay, REQUESTED_VERSION_SELECTOR_WEIGHT, RESOLVED_VERSION_SELECTOR_WEIGHT,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight, VersionSelectors,
    pinned_by_lockfile,
};
pub use publish_time::parse_packument_timestamp;
pub use registries::{
    BUILTIN_REGISTRIES_BY_PREFIX, DEFAULT_JSR_REGISTRY, NeedsFullMetadataFn, NeedsFullMetadataFor,
};
pub use resolve::{
    CurrentPkg, DependencyManifest, GuardExhaustionPolicy, LatestInfo, LatestQuery,
    LockedPackagePolicy, NonDeprecatedAlternative, PackageVersionGuard,
    PackageVersionGuardDecision, PackageVersionGuardError, PackageVersionGuardFuture,
    PkgResolutionId, ResolutionPolicyOptions, ResolutionRefreshOptions, ResolveError,
    ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, ResolvedPackageInfo,
    Resolver, ResolverProjectOptions, ResolverSpecifierOptions, SharedDependencyManifest,
    UpdateBehavior, VersionSelectionOptions, WantedDependency, WorkspacePackage, WorkspacePackages,
    WorkspacePackagesByVersion, resolve_package_version,
};
pub use unsupported_protocol::UnsupportedProtocolError;
pub use verifier::{
    PlannedCanonicalFetches, ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier,
    VerifyCtx, VerifyFuture,
};

mod errors;
mod network_policy;
mod peer_range;
mod policy;
mod preferred_versions;
mod publish_time;
mod registries;
mod resolve;
mod unsupported_protocol;
mod verifier;

#[cfg(test)]
mod tests;

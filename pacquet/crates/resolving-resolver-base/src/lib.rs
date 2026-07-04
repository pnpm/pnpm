//! The resolver-base seam. Two seams live here:
//!
//! 1. **Verifier seam** — [`ResolutionVerifier`] and friends, used by
//!    every resolver-side policy check (today: the npm
//!    `minimumReleaseAge` / `trustPolicy` runner). Pacquet's
//!    lockfile-verification runner depends on the trait without pulling
//!    in any specific resolver.
//!
//! 2. **Dispatcher seam** — [`WantedDependency`], [`ResolveOptions`],
//!    [`ResolveResult`], the [`Resolver`] trait, and the latest-version
//!    companion. Future per-protocol resolvers (npm, git, tarball,
//!    local, jsr, runtimes, named-registry, workspace) implement
//!    [`Resolver`]; the default-resolver dispatcher composes them into
//!    the chain.
//!
//! Both seams sit in the same crate because several types cross over
//! (a verifier needs [`pacquet_lockfile::LockfileResolution`]; a
//! resolver result *also* carries one).

mod publish_time;
mod resolve;
mod verifier;

pub use publish_time::parse_packument_timestamp;
pub use resolve::{
    CurrentPkg, DIRECT_DEP_SELECTOR_WEIGHT, DependencyManifest, EXISTING_VERSION_SELECTOR_WEIGHT,
    LatestInfo, LatestQuery, PackageVersionGuard, PackageVersionGuardDecision,
    PackageVersionGuardError, PackageVersionGuardFuture, PkgResolutionId, PreferredVersions,
    PreferredVersionsOverlay, REQUESTED_VERSION_SELECTOR_WEIGHT, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, SharedDependencyManifest,
    UpdateBehavior, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
    VersionSelectors, WantedDependency, WorkspacePackage, WorkspacePackages,
    WorkspacePackagesByVersion,
};
pub use verifier::{
    ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};

#[cfg(test)]
mod tests;

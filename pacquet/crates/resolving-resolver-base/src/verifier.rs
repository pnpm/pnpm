//! Verifier-side surface of `@pnpm/resolving.resolver-base`. Defines
//! the trait every resolver-side policy check implements, plus the
//! shape used to materialize one rejection.

use std::{future::Future, pin::Pin};

use pacquet_lockfile::{LockfileResolution, PkgName};

/// One verifier's decision about a single `(name, version, resolution)`
/// entry. Mirrors pnpm's
/// [`ResolutionVerification`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L92-L94)
/// discriminated union (`{ ok: true } | { ok: false, code, reason }`).
///
/// Verifiers short-circuit on resolutions outside their protocol by
/// returning [`ResolutionVerification::Ok`]; the runner fans out across
/// every active verifier per candidate and stops at the first
/// [`ResolutionVerification::Err`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionVerification {
    Ok,
    Err {
        /// Verifier-defined error code (e.g.
        /// `MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`). The
        /// install command filters violations by code to decide
        /// downstream UX, so the value is part of the public contract
        /// — verifier crates pin theirs as `&'static str` consts.
        code: &'static str,
        /// Human-readable explanation rendered in the install error
        /// breakdown. Allowed to allocate.
        reason: String,
    },
    /// The registry couldn't be reached to verify the entry
    /// (auth/network/5xx). Unlike [`ResolutionVerification::Err`], this
    /// is not a per-entry policy pick to collect into the batch — the
    /// verification never completed, so the runner aborts the install
    /// with the registry's own fetch error rather than mislabeling a
    /// transport failure as lockfile tampering. Mirrors pnpm, where the
    /// verifier rethrows the underlying `FetchError`.
    FetchFailed {
        /// The registry fetch error, already rendered and stripped of
        /// any credentials embedded in the URL.
        message: String,
    },
}

/// A [`ResolutionVerifier`]'s rejection materialized for one
/// `(name, version, resolution)` entry. Mirrors pnpm's
/// [`ResolutionPolicyViolation`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L145-L151).
///
/// The runner aggregates violations across every active verifier on the
/// loaded lockfile, sorts them by `name@version` for stable output, and
/// caps the rendered breakdown.
///
/// `Eq` is not derived because [`LockfileResolution`] contains
/// `ssri::Integrity`, which is only `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionPolicyViolation {
    pub name: PkgName,
    pub version: String,
    pub resolution: LockfileResolution,
    pub code: &'static str,
    pub reason: String,
}

/// `ctx` argument bundle for [`ResolutionVerifier::verify`]. Mirrors
/// upstream's inline `{ name, version }` object on the verify call.
#[derive(Debug, Clone, Copy)]
pub struct VerifyCtx<'a> {
    pub name: &'a PkgName,
    pub version: &'a str,
}

/// Boxed-future return type for [`ResolutionVerifier::verify`].
///
/// Async-fn-in-trait is stable since Rust 1.75, but `dyn Trait` over a
/// trait that returns `impl Future` is not yet ergonomic without
/// `#[async_trait]` or a manual boxed-future. The runner stores
/// verifiers as `&dyn ResolutionVerifier` so it can fan out across a
/// heterogeneous list (the npm verifier today, future custom
/// verifiers tomorrow); the boxed-future return is the minimal cost
/// for keeping that flexibility while staying off `async-trait`.
pub type VerifyFuture<'a> = Pin<Box<dyn Future<Output = ResolutionVerification> + Send + 'a>>;

/// Optional companion to a resolver factory.
///
/// `verify` inspects the `resolution` shape to decide whether the entry
/// is within its protocol; for entries outside its protocol it should
/// return [`ResolutionVerification::Ok`]. The install side fans out
/// across the verifier list rather than asking a combinator to dispatch.
///
/// `policy` and `can_trust_past_check` describe the verifier's cache
/// contract. Policies from every active verifier are merged into a
/// single shared bag stored alongside the lockfile hash; the
/// install-side verification cache reads them to decide whether a
/// previous run on the same lockfile is still trustworthy under
/// today's policy without re-issuing the registry round-trips that
/// `verify` would. Verifiers that check the same logical policy (e.g.
/// `minimumReleaseAge` across registries) name it the same and share
/// the cache slot.
///
/// Mirrors pnpm's
/// [`ResolutionVerifier`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L113-L131).
pub trait ResolutionVerifier: Send + Sync {
    /// Cheap synchronous filter used before the runner allocates the
    /// verifier future. Returning `false` must be equivalent to
    /// `verify(...)` returning [`ResolutionVerification::Ok`] for the
    /// same entry.
    fn might_verify(&self, _resolution: &LockfileResolution, _ctx: VerifyCtx<'_>) -> bool {
        true
    }

    fn verify<'a>(
        &'a self,
        resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a>;

    /// Snapshot of the policy fields this verifier enforces. Merged
    /// with every other active verifier's `policy` into the cache
    /// record. A field shared across verifiers (same key) should
    /// carry the same value; if it doesn't, the last verifier in the
    /// list wins.
    fn policy(&self) -> &serde_json::Map<String, serde_json::Value>;

    /// Returns `true` when the previously cached policy (the merged
    /// snapshot from the last successful run) can be trusted to still
    /// satisfy what this verifier currently demands. Reads whichever
    /// fields the verifier owns; missing or non-conforming values
    /// (e.g. an older record shape) should return `false`. A loosened
    /// policy can trust a stricter cached run; a tightened policy
    /// cannot.
    fn can_trust_past_check(
        &self,
        cached_policy: &serde_json::Map<String, serde_json::Value>,
    ) -> bool;
}

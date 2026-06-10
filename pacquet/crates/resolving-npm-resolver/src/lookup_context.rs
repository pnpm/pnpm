//! Per-install dedup caches for the npm verifier's
//! publish-timestamp and trust-history lookups.
//!
//! Ports upstream's
//! [`PublishedAtLookupContext`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L387-L433)
//! inline struct. Verifying many `(name, version)` pairs in one
//! install should pay the disk/network costs at most once per
//! `(registry, name)` pair (for package-scoped lookups) or once per
//! `(registry, name, version)` triple (for the final published-at
//! answer). The outer `tokio::sync::Mutex` guards the slot map; each
//! slot is an [`Arc<tokio::sync::OnceCell<T>>`] so two verifier tasks
//! that race for the same key share one in-flight fetch — the second
//! caller awaits the same init future instead of starting a duplicate.
//! Mirrors upstream's `Map<string, Promise<T>>` singleflight pattern;
//! the outer mutex is dropped before the await so unrelated keys stay
//! unblocked.
//!
use std::{collections::HashMap, sync::Arc};

use pacquet_registry::Package;
use tokio::sync::{Mutex, OnceCell};

/// Per-version time map keyed by version string. The verifier only
/// reads the publish timestamp for a specific version, so storing
/// `String` per entry is enough — the rest of the package's `time`
/// payload (`created`, `modified`, the reserved `unpublished` key)
/// is irrelevant to the policy.
pub(crate) type PublishedAtTimeMap = HashMap<String, String>;

/// The fields the verifier needs from the packument: the
/// package-level last-modified timestamp and a per-version map of
/// `dist.tarball`. The map's keys double as the set of version names
/// the abbreviated-modified shortcut checks; the values feed the
/// tarball-URL binding. Projected off the abbreviated packument so the
/// verifier can keep the rest of the document GC-able after the
/// lookup — the full document runs to hundreds of KB per package and
/// OOMs CI runners on multi-thousand entry installs (see
/// [`#11860`](https://github.com/pnpm/pnpm/issues/11860)); only the
/// short tarball-URL strings are retained.
///
/// Mirrors upstream's
/// [`AbbreviatedMetaProjection`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L709-L712).
#[derive(Debug, Default, Clone)]
pub(crate) struct AbbreviatedMetaProjection {
    pub modified: Option<String>,
    /// version → `dist.tarball`; key presence means the version is published.
    pub version_tarballs: Option<HashMap<String, String>>,
    /// version → `dist` work statistics (`unpackedSize`, `fileCount`),
    /// for the versions whose registry published either. Not part of
    /// upstream's projection — carried so the verifier can surface
    /// tarball work estimates to fetch scheduling (the verifier's
    /// [`ObservedDistStats`] sink) without a second metadata
    /// round-trip.
    ///
    /// [`ObservedDistStats`]: crate::ObservedDistStats
    pub version_dist_stats: Option<HashMap<String, crate::DistStats>>,
}

/// Slot map of singleflight cells. Outer mutex guards lookup/insert;
/// each cell is shared so concurrent verifier tasks that race on the
/// same key wait on a single in-flight init.
pub(crate) type SingleflightMap<Value> = Mutex<HashMap<String, Arc<OnceCell<Value>>>>;

/// Per-install dedup of the lookups the verifier issues. Mirrors
/// upstream's
/// [`PublishedAtLookupContext`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L387-L433).
///
/// Each `HashMap` is keyed by a per-cache string composed from
/// `registry`, `name`, and (for `published_at`) `version`. Upstream
/// joins them with a `\x00` separator to sidestep any collision with
/// legal URL/name characters; we keep the same convention so cache
/// keys remain identical across stacks.
#[derive(Debug, Default)]
pub(crate) struct PublishedAtLookupContext {
    pub published_at: SingleflightMap<Result<Option<String>, String>>,
    pub full_meta: SingleflightMap<Result<Option<Arc<PublishedAtTimeMap>>, String>>,
    pub full_meta_for_trust: SingleflightMap<Result<Arc<Package>, String>>,
    pub abbreviated_meta: SingleflightMap<Option<AbbreviatedMetaProjection>>,
    pub local_meta: SingleflightMap<Option<Arc<PublishedAtTimeMap>>>,
}

impl PublishedAtLookupContext {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// `\x00`-joined cache key for `(registry, name)` package-scoped
/// lookups. Matches upstream's `${registry}\x00${name}` template.
pub(crate) fn package_key(registry: &str, name: &str) -> String {
    format!("{registry}\x00{name}")
}

/// `\x00`-joined cache key for `(registry, name, version)` lookups.
/// Matches upstream's `${registry}\x00${name}\x00${version}`.
pub(crate) fn version_key(registry: &str, name: &str, version: &str) -> String {
    format!("{registry}\x00{name}\x00{version}")
}

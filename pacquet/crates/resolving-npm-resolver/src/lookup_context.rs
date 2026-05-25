//! Per-install dedup caches for the npm verifier's
//! publish-timestamp and trust-history lookups.
//!
//! Ports upstream's
//! [`PublishedAtLookupContext`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L387-L433)
//! inline struct. Verifying many `(name, version)` pairs in one
//! install should pay the disk/network costs at most once per
//! `(registry, name)` pair (for package-scoped lookups) or once per
//! `(registry, name, version)` triple (for the final published-at
//! answer). The maps live behind `tokio::sync::Mutex` so the
//! buffer-unordered fan-out the lockfile-verification runner uses
//! can share one context across concurrent tasks without contending
//! on the publishing record itself.
//!
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use pacquet_registry::Package;
use tokio::sync::Mutex;

/// Per-version time map keyed by version string. The verifier only
/// reads the publish timestamp for a specific version, so storing
/// `String` per entry is enough — the rest of the package's `time`
/// payload (`created`, `modified`, the reserved `unpublished` key)
/// is irrelevant to the policy.
pub(crate) type PublishedAtTimeMap = HashMap<String, String>;

/// Two fields the abbreviated-modified shortcut needs from the
/// packument: the package-level last-modified timestamp and the set
/// of version names the registry currently lists. Projected off the
/// abbreviated packument so the verifier can keep the rest of the
/// document GC-able after the lookup — the full document runs to
/// hundreds of KB per package and OOMs CI runners on multi-thousand
/// entry installs (see [`#11860`](https://github.com/pnpm/pnpm/issues/11860)).
///
/// Mirrors upstream's
/// [`AbbreviatedMetaProjection`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L709-L712).
#[derive(Debug, Default, Clone)]
pub(crate) struct AbbreviatedMetaProjection {
    pub modified: Option<String>,
    pub version_names: Option<HashSet<String>>,
}

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
    pub published_at: Mutex<HashMap<String, Option<String>>>,
    pub full_meta: Mutex<HashMap<String, Option<Arc<PublishedAtTimeMap>>>>,
    pub full_meta_for_trust: Mutex<HashMap<String, Result<Arc<Package>, String>>>,
    pub abbreviated_meta: Mutex<HashMap<String, Option<AbbreviatedMetaProjection>>>,
    pub local_meta: Mutex<HashMap<String, Option<Arc<PublishedAtTimeMap>>>>,
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

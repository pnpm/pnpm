//! Which reads a pick may try before the registry, decided from the request,
//! the policy, and the mirror's header line alone. The reads themselves
//! check the mirror's content: whether it holds the version and whether its
//! pick agrees.

use chrono::{DateTime, TimeDelta, Utc};
use pnpm_config::{NetworkPolicy, TrustPolicy};

use crate::{
    PackagePickPolicy, metadata_cache_policy::mirror_may_answer, mirror::MetaHeaders,
    pick_package_from_meta::RegistryPackageSpecType,
};

/// Registries that omit `ETag` cannot answer `If-None-Match` with 304, so a
/// warm revalidation downloads the whole packument. A public mirror younger
/// than this and stored without an `ETag` is reused for a range the cache can
/// already satisfy. The public npm registry sends `ETag`s, so it keeps
/// conditional revalidation. After this age the mirror is fetched again.
pub(crate) const UNVALIDATED_MIRROR_MAX_AGE: TimeDelta = TimeDelta::minutes(5);

/// What [`MirrorShortcuts::decide`] reads.
#[derive(Clone, Copy)]
pub(super) struct RouteInputs<'a> {
    pub network: NetworkPolicy,
    pub request: RouteRequest,
    /// Read last, and only by a shortcut that is otherwise open: the
    /// release-age exclusion is a policy lookup per call.
    pub policy: &'a PackagePickPolicy<'a>,
    pub pkg_name: &'a str,
    pub public_scope: bool,
    /// The mirror's header line, `None` when there is no mirror.
    pub mirror: Option<&'a MetaHeaders>,
}

/// The parts of the pick request a route reads.
#[derive(Debug, Clone, Copy)]
pub(super) struct RouteRequest {
    pub spec_type: RegistryPackageSpecType,
    pub include_latest_tag: bool,
    pub update_checksums: bool,
    pub refresh_metadata: bool,
    pub pick_lowest_version: bool,
    pub has_blocked_versions: bool,
}

impl RouteInputs<'_> {
    /// Whether a range pick could be settled from the mirror at all: every
    /// option that makes the answer depend on fresh metadata rules it out.
    fn range_pick_is_stable(&self) -> bool {
        let request = self.request;
        request.spec_type == RegistryPackageSpecType::Range
            && !self.network.reads_cache_first()
            && !request.pick_lowest_version
            && !request.include_latest_tag
            && !request.update_checksums
            && self.policy.trust_policy != Some(TrustPolicy::NoDowngrade)
            && !request.has_blocked_versions
            && !self.policy.release_age_applies_to(self.pkg_name)
    }

    /// The `minimumReleaseAge` cutoff when a mirror written at or after it
    /// may answer the pick: such a mirror cannot be missing a mature version.
    /// The last mirror read, asked only after [`MirrorShortcuts::decide`]
    /// let the mirror answer and the reads it returned missed.
    pub(super) fn written_after(&self) -> Option<DateTime<Utc>> {
        let cutoff = self.policy.published_by?;
        let range_offline = self.network.forbids_registry()
            && self.request.spec_type == RegistryPackageSpecType::Range;
        (!range_offline && self.policy.release_age_applies_to(self.pkg_name)).then_some(cutoff)
    }
}

/// The mirror reads a pick tries before taking the fetch permit, in order.
/// [`RouteInputs::written_after`] is the last of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MirrorShortcuts {
    /// A pinned version the mirror holds.
    pub exact_version: bool,
    /// A range whose lockfile version dominates every other selector.
    pub dominant_lockfile_version: bool,
    /// A public mirror stored without an `ETag`, while
    /// [`unvalidated_mirror_is_fresh`] holds. A private route still
    /// contacts the registry so a `401` is not hidden behind the mirror.
    pub unvalidated_mirror: bool,
}

impl MirrorShortcuts {
    /// `None` when the mirror may not answer the pick at all.
    pub(super) fn decide(inputs: &RouteInputs<'_>) -> Option<Self> {
        let RouteInputs {
            network,
            request,
            public_scope,
            mirror,
            ..
        } = *inputs;
        if !mirror_may_answer(network, mirror) {
            return None;
        }
        if request.refresh_metadata && !network.reads_cache_first() {
            return None;
        }
        let stable_range = inputs.range_pick_is_stable();
        let mirror_has_no_etag =
            mirror.is_some_and(|headers| headers.etag.as_deref().is_none_or(str::is_empty));
        Some(MirrorShortcuts {
            exact_version: request.spec_type == RegistryPackageSpecType::Version
                && !request.include_latest_tag
                && !request.update_checksums,
            dominant_lockfile_version: stable_range,
            unvalidated_mirror: stable_range && public_scope && mirror_has_no_etag,
        })
    }
}

/// What a pick from an in-memory entry must pass before it is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheHitCheck {
    Serve,
    /// Serve only the version the entry is stable on, see
    /// [`crate::pick_package_from_meta::pick_stable_cached_range_version`].
    StableRangePick,
    /// Fall through to the reads and the fetch that replace the entry.
    Revalidate,
}

impl CacheHitCheck {
    /// `registry_verified` is false for an entry parsed from the mirror
    /// without a registry round-trip (see
    /// [`super::PackageMetaCache::set_unverified`]). `picked` is whether the
    /// entry produced a pick. Offline, every pick is served: there is nothing
    /// to revalidate against.
    pub(super) fn decide(inputs: &RouteInputs<'_>, registry_verified: bool, picked: bool) -> Self {
        let RouteInputs { network, request, .. } = *inputs;
        if network.forbids_registry() || registry_verified {
            return CacheHitCheck::Serve;
        }
        if request.refresh_metadata && !network.prefers_reachable_cache() {
            return CacheHitCheck::Revalidate;
        }
        if !picked {
            return CacheHitCheck::Revalidate;
        }
        // These picks answer the same from a stale mirror.
        if network.prefers_reachable_cache()
            || request.pick_lowest_version
            || request.spec_type == RegistryPackageSpecType::Version
        {
            return CacheHitCheck::Serve;
        }
        if inputs.range_pick_is_stable() {
            CacheHitCheck::StableRangePick
        } else {
            CacheHitCheck::Revalidate
        }
    }
}

/// Whether a mirror written at `mtime` is young enough for
/// [`MirrorShortcuts::unvalidated_mirror`].
///
/// The age is compared in both directions. A mirror dated far in the future,
/// for example after the clock was set back, has an unknown age and is not
/// reused. A few milliseconds of skew between the file system and the clock
/// are tolerated.
pub(super) fn unvalidated_mirror_is_fresh(mtime: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(mtime).abs() < UNVALIDATED_MIRROR_MAX_AGE
}

/// Whether a pick reads the mirror after taking the fetch permit, before it
/// fetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiskFirst {
    Never,
    Always,
    /// A lowest-version pick reads the mirror unless the registry forbade
    /// caching it (see [`mirror_may_answer`]).
    UnlessUncacheable,
}

impl DiskFirst {
    pub(super) fn decide(
        network: NetworkPolicy,
        pick_lowest_version: bool,
        refresh_metadata: bool,
    ) -> Self {
        if network.reads_cache_first() {
            DiskFirst::Always
        } else if refresh_metadata {
            DiskFirst::Never
        } else if pick_lowest_version {
            DiskFirst::UnlessUncacheable
        } else {
            DiskFirst::Never
        }
    }
}

/// Whether a failed fetch may fall back to the mirror.
///
/// A private route must fail closed on a `401`/`403`/private-`404`: a revoked
/// credential or a hidden private package must not keep serving the last
/// cached packument, even from its own (same-namespace) mirror. Only a
/// transport failure (`5xx`/timeout/network) falls back. A public route falls
/// back on any error.
pub(super) fn fetch_failure_falls_back(public_scope: bool, access_denied: bool) -> bool {
    public_scope || !access_denied
}

#[cfg(test)]
mod tests;

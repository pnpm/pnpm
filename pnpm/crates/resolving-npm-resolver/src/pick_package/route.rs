//! Which reads a pick may try before the registry, decided from the request,
//! the policy, and the mirror's header line alone. The reads themselves
//! check the mirror's content: whether it holds the version and whether its
//! pick agrees.

use chrono::{DateTime, TimeDelta, Utc};
use pnpm_config::NetworkPolicy;

use crate::{
    metadata_cache_policy::mirror_may_answer, mirror::MetaHeaders,
    pick_package_from_meta::RegistryPackageSpecType,
};

/// Registries that omit `ETag` cannot answer `If-None-Match` with 304, so a
/// warm revalidation downloads the whole packument. A public mirror younger
/// than this and stored without an `ETag` is reused for a range the cache can
/// already satisfy. The public npm registry sends `ETag`s, so it keeps
/// conditional revalidation. After this age the mirror is fetched again.
pub(crate) const UNVALIDATED_MIRROR_MAX_AGE: TimeDelta = TimeDelta::minutes(5);

/// What [`MirrorShortcuts::decide`] reads.
#[derive(Debug, Clone, Copy)]
pub(super) struct RouteInputs<'a> {
    pub network: NetworkPolicy,
    pub request: RouteRequest,
    pub policy: RoutePolicy,
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

/// The parts of the pick policy a route reads.
#[derive(Debug, Clone, Copy)]
pub(super) struct RoutePolicy {
    /// `minimumReleaseAge` narrows this package's versions.
    pub release_age_applies: bool,
    pub published_by: Option<DateTime<Utc>>,
    pub trust_no_downgrade: bool,
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
            && !self.policy.release_age_applies
            && !self.policy.trust_no_downgrade
            && !request.has_blocked_versions
    }
}

/// The mirror reads a pick tries before taking the fetch permit, in order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct MirrorShortcuts {
    /// A pinned version the mirror holds.
    pub exact_version: bool,
    /// A range whose lockfile version dominates every other selector.
    pub dominant_lockfile_version: bool,
    /// A public mirror stored without an `ETag`, while
    /// [`unvalidated_mirror_is_fresh`] holds. A private route still
    /// contacts the registry so a `401` is not hidden behind the mirror.
    pub unvalidated_mirror: bool,
    /// The `minimumReleaseAge` cutoff: a mirror written at or after it
    /// cannot be missing a mature version.
    pub written_after: Option<DateTime<Utc>>,
}

impl MirrorShortcuts {
    pub(super) fn decide(inputs: &RouteInputs<'_>) -> Self {
        let RouteInputs {
            network,
            request,
            policy,
            public_scope,
            mirror,
        } = *inputs;
        if !mirror_may_answer(network, mirror) {
            return MirrorShortcuts::default();
        }
        let stable_range = inputs.range_pick_is_stable();
        let mirror_has_no_etag =
            mirror.is_some_and(|headers| headers.etag.as_deref().is_none_or(str::is_empty));
        let range_offline =
            network.forbids_registry() && request.spec_type == RegistryPackageSpecType::Range;
        MirrorShortcuts {
            exact_version: request.spec_type == RegistryPackageSpecType::Version
                && !request.include_latest_tag
                && !request.update_checksums,
            dominant_lockfile_version: stable_range,
            unvalidated_mirror: stable_range
                && !request.refresh_metadata
                && public_scope
                && mirror_has_no_etag,
            written_after: policy.published_by.filter(|_| {
                policy.release_age_applies && !range_offline
            }),
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
    pub(super) fn decide(network: NetworkPolicy, pick_lowest_version: bool) -> Self {
        if network.reads_cache_first() {
            DiskFirst::Always
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

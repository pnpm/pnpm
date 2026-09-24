use super::{
    AllVersionsBlockedError, Arc, GuardExhaustionPolicy, GuardRepickLimitError, Package,
    PackageMetaCache, PackageVersion, PackageVersionGuardDecision, PickPackageContext,
    PickPackageError, PickPackageOptions, RegistryPackageSpec, RegistryResponseError,
    RegistryResponseErrorOptions, ResolveError, pick_package, redact_and_sanitize,
    registry_response_status, to_registry_url,
};
use crate::trust_checks::{TrustCheckOptions, TrustViolation, fail_if_trust_downgraded};

/// Picker output threaded through to [`build_resolve_result`](super::resolution_result::build_resolve_result).
/// `meta` is shared as [`Arc<Package>`] to avoid deep-cloning the
/// full packument (with all versions) on every pick.
pub(crate) struct PickedFromRegistry {
    pub(crate) meta: std::sync::Arc<Package>,
    pub(crate) version: std::sync::Arc<PackageVersion>,
}

/// Outcome of a registry pick.
pub(crate) enum RegistryPick {
    Picked(PickedFromRegistry),
    /// The registry served the packument but no published version satisfied
    /// the request. The packument comes along because
    /// [`NoMatchingVersionError`](pnpm_resolving_resolver_base::NoMatchingVersionError) reports what *is* published.
    NoMatchingVersion(std::sync::Arc<Package>),
}

pub(crate) struct PickFromRegistryOptions<'a> {
    pub registry: &'a str,
    pub spec: &'a RegistryPackageSpec,
    pub preferred_version_selectors: Option<&'a pnpm_resolving_resolver_base::VersionSelectors>,
    pub pick_lowest_version: bool,
    pub include_latest_tag: bool,
    pub package_version_guard:
        Option<&'a Arc<dyn pnpm_resolving_resolver_base::PackageVersionGuard>>,
    /// The `trustPolicy: no-downgrade` check, when it applies to this pick.
    /// A candidate it rejects as a trust downgrade is set aside the way a
    /// guard rejection is, and the first downgrade is the error when no
    /// candidate is left.
    pub trust_check: Option<TrustCheckOptions<'a>>,
    pub policy: crate::PackagePickPolicy<'a>,
    pub request: crate::MetadataPickRequest,
}

/// Upper bound on guard rejections for one package before the resolver
/// gives up. Far beyond any realistic run of consecutive blocked
/// versions, so it only fires on a pathological/hostile packument.
pub(super) const GUARD_REPICK_LIMIT: usize = 1000;

pub(super) fn pick_options<'o>(
    opts: &'o PickFromRegistryOptions<'o>,
    blocked_versions: &'o std::collections::HashSet<String>,
) -> PickPackageOptions<'o> {
    PickPackageOptions {
        registry: opts.registry,
        preferred_version_selectors: opts.preferred_version_selectors,
        pick_lowest_version: opts.pick_lowest_version,
        include_latest_tag: opts.include_latest_tag,
        blocked_versions: (!blocked_versions.is_empty()).then_some(blocked_versions),
        policy: opts.policy,
        request: opts.request,
    }
}

pub(super) fn all_versions_blocked(name: String, reason: String) -> ResolveError {
    Box::new(AllVersionsBlockedError { name, reason })
}

pub(crate) async fn pick_from_registry_with_guard<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    opts: PickFromRegistryOptions<'_>,
) -> Result<RegistryPick, ResolveError> {
    let mut blocked_versions = std::collections::HashSet::new();
    let mut last_rejection: Option<String> = None;
    // The first candidate the guard turned down, i.e. the one the picker
    // would have returned with no guard at all.
    let mut first_rejected: Option<PickedFromRegistry> = None;
    let mut trust_rejections = TrustRejections::default();
    loop {
        let pick_result = pick_package(ctx, opts.spec, &pick_options(&opts, &blocked_versions))
            .await
            .map_err(|err| map_pick_error(ctx, &opts, err))?;

        let Some(version) = pick_result.picked_package else {
            // No candidate left. With no prior guard rejection this is the
            // ordinary "no matching version" outcome; once the guard has
            // rejected every match, the guard's own policy decides, and a
            // failure names the guard rather than blaming the range the user
            // wrote.
            return match last_rejection {
                Some(reason) => exhausted(&opts, first_rejected, reason, all_versions_blocked),
                None => match trust_rejections.first_downgrade {
                    Some(downgrade) => Err(Box::new(downgrade)),
                    None => Ok(RegistryPick::NoMatchingVersion(pick_result.meta)),
                },
            };
        };
        let version_str = version.version.to_string();

        if let Some(trust_check) = &opts.trust_check {
            // The first pick runs with nothing blocked, so its packument
            // holds the whole publish history every candidate is judged by.
            let full_meta =
                trust_rejections.full_meta.get_or_insert_with(|| Arc::clone(&pick_result.meta));
            match fail_if_trust_downgraded(full_meta, &version_str, trust_check) {
                Ok(()) => {}
                Err(downgrade @ TrustViolation::TrustDowngrade { .. }) => {
                    let blocked_key =
                        blocked_packument_key(&pick_result.meta, &version, &version_str);
                    if repick_limit_reached(&mut blocked_versions, blocked_key).is_some() {
                        return Err(Box::new(trust_rejections.first_downgrade.unwrap_or(
                            downgrade,
                        )));
                    }
                    trust_rejections.rejected_versions.push(version_str);
                    trust_rejections.first_downgrade.get_or_insert(downgrade);
                    continue;
                }
                Err(violation) => return Err(Box::new(violation)),
            }
        }

        let Some(guard) = opts.package_version_guard else {
            return Ok(trust_rejections.picked(&opts.spec.name, pick_result.meta, version));
        };

        let PackageVersionGuardDecision::Reject { reason } =
            guard.check(&opts.spec.name, &version_str).await?
        else {
            return Ok(trust_rejections.picked(&opts.spec.name, pick_result.meta, version));
        };
        log_guard_rejection(&opts.spec.name, &version_str, &reason);
        // Block by the *packument key*, which the next pick filters on. It
        // usually equals the parsed manifest version, but a registry that
        // serves a key differing from the manifest's `version` field would
        // otherwise never get the candidate excluded — re-selecting it
        // forever and wrongly reporting every version blocked when a lower
        // one is still fine.
        let blocked_key = blocked_packument_key(&pick_result.meta, &version, &version_str);
        if let Some(stop) = repick_limit_reached(&mut blocked_versions, blocked_key) {
            return exhausted(&opts, first_rejected, reason, stop.into_error());
        }
        last_rejection = Some(reason);
        first_rejected.get_or_insert(PickedFromRegistry { meta: pick_result.meta, version });
    }
}

/// Candidates the `trustPolicy: no-downgrade` check set aside during one
/// guarded pick.
#[derive(Default)]
struct TrustRejections {
    full_meta: Option<Arc<Package>>,
    first_downgrade: Option<TrustViolation>,
    rejected_versions: Vec<String>,
}

impl TrustRejections {
    /// The accepted candidate. After a trust fallback it carries the
    /// packument from before any version was set aside, so the result
    /// reports the same `latest` it would have without the fallback.
    fn picked(self, name: &str, meta: Arc<Package>, version: Arc<PackageVersion>) -> RegistryPick {
        let meta = match self.full_meta {
            Some(full_meta) if !self.rejected_versions.is_empty() => {
                warn_once_on_trust_downgrade_fallback(
                    name,
                    &version.version.to_string(),
                    &self.rejected_versions,
                );
                full_meta
            }
            _ => meta,
        };
        RegistryPick::Picked(PickedFromRegistry { meta, version })
    }
}

const MAX_WARNED_TRUST_DOWNGRADE_FALLBACKS: usize = 1024;
static WARNED_TRUST_DOWNGRADE_FALLBACKS: std::sync::LazyLock<
    std::sync::Mutex<indexmap::IndexSet<String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(indexmap::IndexSet::new()));

fn warn_once_on_trust_downgrade_fallback(name: &str, picked: &str, rejected_versions: &[String]) {
    let key = format!("{name}@{picked}<{}", rejected_versions.join(","));
    let mut warned =
        WARNED_TRUST_DOWNGRADE_FALLBACKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.contains(&key) {
        return;
    }
    if warned.len() >= MAX_WARNED_TRUST_DOWNGRADE_FALLBACKS {
        warned.shift_remove_index(0);
    }
    warned.insert(key);
    let skipped = rejected_versions
        .iter()
        .map(|version| format!("{name}@{version}"))
        .collect::<Vec<_>>()
        .join(", ");
    tracing::warn!(
        target: "pnpm_resolving_npm_resolver",
        "Skipped trust downgrades rejected by trustPolicy: {skipped}. Resolved {name}@{picked} instead.",
    );
}

/// Why the guard loop stops re-picking.
pub(super) enum RepickStop {
    /// The picker re-selected a key that was already blocked, so it cannot
    /// be excluded: every match really is blocked.
    AllBlocked,
    /// The cap on re-picks was reached. It bounds work rather than proving
    /// every version is blocked, so it carries its own error for a guard
    /// that demands a clean candidate.
    LimitReached,
}

impl RepickStop {
    pub(super) fn into_error(self) -> fn(String, String) -> ResolveError {
        match self {
            RepickStop::AllBlocked => {
                |name, reason| Box::new(AllVersionsBlockedError { name, reason })
            }
            RepickStop::LimitReached => |name, reason| {
                Box::new(GuardRepickLimitError { name, limit: GUARD_REPICK_LIMIT, reason })
            },
        }
    }
}

/// Record one blocked key, reporting why the loop must stop when it must.
///
/// Each rejection re-runs the picker over the packument, so an unbounded run
/// is O(versions²). The cap sits well above any real run of consecutive
/// rejected versions, to bound the work a hostile packument can force.
pub(super) fn repick_limit_reached(
    blocked_versions: &mut std::collections::HashSet<String>,
    blocked_key: String,
) -> Option<RepickStop> {
    if !blocked_versions.insert(blocked_key) {
        return Some(RepickStop::AllBlocked);
    }
    (blocked_versions.len() >= GUARD_REPICK_LIMIT).then_some(RepickStop::LimitReached)
}

/// The packument key for a picked version, so the guard loop can block the
/// exact entry the next pick filters on. Fast-paths the common case where
/// the parsed manifest version is itself the key; only falls back to
/// locating the key by identity when a registry served a mismatched key.
pub(super) fn blocked_packument_key(
    meta: &Package,
    picked: &Arc<PackageVersion>,
    version_str: &str,
) -> String {
    if meta.versions.contains_key(version_str) {
        return version_str.to_string();
    }
    meta.versions
        .keys()
        .find(|key| meta.versions.get(key).is_some_and(|candidate| Arc::ptr_eq(&candidate, picked)))
        .cloned()
        .unwrap_or_else(|| version_str.to_string())
}

/// Answer a request the picker gave up on — every matching version rejected,
/// or the re-pick cap reached first — per the guard's
/// [`GuardExhaustionPolicy`]. `fail` builds the error naming which of the two
/// it was, for a guard whose rejections are a hard requirement.
pub(super) fn exhausted(
    opts: &PickFromRegistryOptions<'_>,
    first_rejected: Option<PickedFromRegistry>,
    reason: String,
    fail: impl FnOnce(String, String) -> ResolveError,
) -> Result<RegistryPick, ResolveError> {
    let accepts_rejected = opts.package_version_guard.is_some_and(|guard| {
        guard.exhaustion_policy() == GuardExhaustionPolicy::AcceptRejected
    });
    match first_rejected.filter(|_| accepts_rejected) {
        Some(picked) => {
            tracing::debug!(
                target: "pnpm_resolving_npm_resolver",
                name = %opts.spec.name,
                version = %picked.version.version,
                reason = %reason,
                "every matching version was rejected by the resolver guard; keeping the unguarded pick",
            );
            Ok(RegistryPick::Picked(picked))
        }
        None => Err(fail(opts.spec.name.clone(), reason)),
    }
}

/// Box a picker failure for the resolver chain, restating a non-2xx registry
/// answer as [`RegistryResponseError`]. Without that the transport-level
/// message ("HTTP status client error (404 Not Found) for url ...") is all the
/// user sees: no `ERR_PNPM_FETCH_404`, and no hint that a 404 from a private
/// registry is often really an authorization failure.
pub(super) fn map_pick_error<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    opts: &PickFromRegistryOptions<'_>,
    error: PickPackageError,
) -> ResolveError {
    let Some(status) = registry_response_status(&error) else {
        return Box::new(error);
    };
    let url = to_registry_url(opts.registry, &opts.spec.name);
    // Look the credential up with the URL the fetch itself used. An inline
    // `user:pass@` is exactly what `AuthHeaders` turns into a Basic header, so
    // redacting before the lookup would report "no authorization header was
    // set" for the registries that most certainly carry one.
    let auth_header_value =
        ctx.metadata.http.auth_headers.for_url_with_package(&url, Some(&opts.spec.name));
    Box::new(RegistryResponseError::new(RegistryResponseErrorOptions {
        url: &redact_and_sanitize(&url),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default(),
        pkg_name: &opts.spec.name,
        auth_header_value: auth_header_value.as_deref(),
    }))
}

pub(super) fn log_guard_rejection(name: &str, version_str: &str, reason: &str) {
    tracing::debug!(
        target: "pnpm_resolving_npm_resolver",
        name = %name,
        version = %version_str,
        reason = %reason,
        "package version rejected by resolver guard",
    );
}

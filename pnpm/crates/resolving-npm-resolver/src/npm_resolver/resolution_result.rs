use super::{
    Algorithm, Arc, DateTime, Integrity, InvalidTarballIntegrityError, LockfileResolution,
    MINIMUM_RELEASE_AGE_VIOLATION_CODE, Package, PackageDistribution, PackageVersion,
    PackageVersionPolicy, PickedFromRegistry, PkgName, PkgNameVer, PkgResolutionId, RangeSpecStyle,
    RegistryPackageSpec, RegistryResponseError, RegistryRevisionSelector,
    ResolutionPolicyViolation, ResolveError, ResolveOptions, ResolveResult, TarballResolution,
    TarballRevision, TrustCheckOptions, TrustPolicy, Utc, Version, WantedDependency,
    fail_if_trust_downgraded, parse_packument_timestamp, select_package_revision, tarball_revision,
};

/// Input bundle for [`build_resolve_result`]. Grouped so the
/// 9-field signature stays a struct literal at the (3) call sites
/// instead of a positional argument list that clippy flags as
/// `too_many_arguments` (and that's painful to extend when the
/// next field lands).
pub(crate) struct BuildResolveResult<'a> {
    pub meta: &'a Package,
    pub picked: &'a PackageVersion,
    pub spec: &'a RegistryPackageSpec,
    pub alias: Option<&'a str>,
    pub resolved_via: &'a str,
    pub registry: &'a str,
    /// `Some(alias)` when the caller resolves from a named registry and
    /// registry-qualified ids are enabled — the minted id then becomes
    /// `<name>@<alias>:<version>` (lockfile format 12.0).
    pub registry_name: Option<&'a str>,
    pub published_by: Option<DateTime<Utc>>,
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
    pub picked_manifest_cache: &'a crate::PickedManifestCache,
    /// The manifest-ready specifier for `picked`, rendered in whichever
    /// shape the caller's protocol round-trips through, or `None` when
    /// the caller did not ask for one
    /// (`ResolveOptions::calc_specifier`).
    pub calculated_specifier: Option<String>,
}

pub(crate) fn build_resolve_result(
    args: BuildResolveResult<'_>,
) -> Result<ResolveResult, ResolveError> {
    let picked = select_package_revision(args.picked, args.spec, args.registry)?;
    let picked = picked.as_ref();
    let pkg_name =
        PkgName::parse(picked.name.as_str()).map_err(|err| Box::new(err) as ResolveError)?;
    let version_str = picked.version.to_string();
    let name_ver = PkgNameVer::new(pkg_name.clone(), picked.version.clone());
    let (resolution, revision) = picked_tarball_resolution(picked, args.registry)?;
    let published_at = args.meta.published_at(&version_str).map(str::to_string);
    let manifest = args.manifest_for_revision(picked, &version_str, revision)?;
    Ok(ResolveResult {
        id: resolution_id(args.registry_name, picked, &name_ver),
        name_ver: Some(name_ver),
        latest: latest_allowed_by_policy(args.meta, args.published_by, args.published_by_exclude)
            .map(str::to_string),
        published_at: published_at.clone(),
        manifest: Some(manifest),
        policy_violation: detect_min_release_age_violation(
            &pkg_name,
            &version_str,
            published_at.as_deref(),
            &resolution,
            args.published_by,
            args.published_by_exclude,
        ),
        resolution,
        resolved_via: args.resolved_via.to_string(),
        normalized_bare_specifier: args
            .spec
            .normalized_bare_specifier
            .clone()
            .or(args.calculated_specifier),
        alias: args.alias.map(str::to_string),
    })
}

pub(super) fn calculated_specifier(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    picked: &PickedFromRegistry,
) -> Option<String> {
    revision_specifier(wanted_dependency, opts, spec, None, &spec.name, &picked.version.version)
        .or_else(|| {
            calc_specifier_from(wanted_dependency, opts, spec).map(
                |(bare_specifier, default_pin)| {
                    crate::calc_specifier(
                        bare_specifier,
                        wanted_dependency.prev_specifier.as_deref(),
                        wanted_dependency.alias.as_deref(),
                        &picked.version,
                        default_pin,
                    )
                },
            )
        })
}

pub(super) fn resolution_id(
    registry_name: Option<&str>,
    picked: &PackageVersion,
    name_ver: &PkgNameVer,
) -> PkgResolutionId {
    match registry_name {
        Some(registry_name) => {
            PkgResolutionId::from(format!("{}@{registry_name}:{}", picked.name, picked.version))
        }
        None => name_ver.into(),
    }
}

/// Dedupe `serde_json::to_value(picked)` across picks of the same
/// `(registry, pkg_name, version)` triple — see [`PickedManifestCache`](crate::pick_package::PickedManifestCache)
/// for the rationale. The cache is shared across the npm / JSR /
/// named-registry resolvers, so the key has to scope by `registry` too;
/// two registries may serve different artifacts under the same
/// `name@version`, and collapsing them would hand the second registry's
/// resolver the first registry's manifest — wrong dependency graph,
/// wrong peers, wrong lockfile metadata. Matches `meta_cache`'s
/// `{registry}\x00{name}` scoping shape.
pub(super) fn cached_manifest(
    cache: &crate::PickedManifestCache,
    key: String,
    picked: &PackageVersion,
) -> Result<Arc<serde_json::Value>, ResolveError> {
    if let Some(cached) = cache.get(&key) {
        return Ok(Arc::clone(cached.value()));
    }
    let arc = Arc::new(serde_json::to_value(picked).map_err(|err| Box::new(err) as ResolveError)?);
    cache.insert(key, Arc::clone(&arc));
    Ok(arc)
}

/// The integrity a registry version's `dist` pins its tarball with.
///
/// A registry predating subresource integrity publishes only the legacy
/// `dist.shasum` hex digest, which pins the bytes just as well, so it is
/// promoted to its `sha1-` SRI form. `None` when the version pins
/// nothing at all.
pub(super) fn dist_integrity(
    dist: &PackageDistribution,
) -> Result<Option<Integrity>, ResolveError> {
    if let Some(integrity) = &dist.integrity {
        return Ok(Some(integrity.clone()));
    }
    let Some(shasum) = dist.shasum.as_deref().filter(|shasum| !shasum.is_empty()) else {
        return Ok(None);
    };
    Integrity::from_hex(shasum, Algorithm::Sha1).map(Some).map_err(|_| {
        Box::new(InvalidTarballIntegrityError::new(&dist.tarball, shasum)) as ResolveError
    })
}

/// The `(specifier, pin)` pair a manifest-ready specifier is computed
/// from, or `None` when there is nothing to compute: the caller did not
/// ask for one (`ResolveOptions::calc_specifier`), or `spec` already
/// carries the text the entry has to keep — a registry-host tarball URL,
/// which [`build_resolve_result`] prefers over anything computed here.
/// Caret is the fallback pin, matching pnpm's default save prefix.
pub(crate) fn calc_specifier_from<'a>(
    wanted_dependency: &'a WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
) -> Option<(&'a str, RangeSpecStyle)> {
    if !opts.calc_specifier || spec.normalized_bare_specifier.is_some() {
        return None;
    }
    let bare_specifier = wanted_dependency.bare_specifier.as_deref()?;
    Some((bare_specifier, opts.range_spec_style.unwrap_or(RangeSpecStyle::Major)))
}

pub(crate) fn revision_specifier(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    prefix: Option<&str>,
    package_name: &str,
    version: &Version,
) -> Option<String> {
    if !opts.calc_specifier || spec.normalized_bare_specifier.is_some() {
        return None;
    }
    let RegistryRevisionSelector::Valid(revision) = spec.revision.as_ref()? else {
        return None;
    };
    let target = format!("{version}+r{revision}");
    let alias_matches =
        wanted_dependency.alias.as_deref().is_none_or(|alias| alias == package_name);
    match prefix {
        Some(prefix) if alias_matches => Some(format!("{prefix}{target}")),
        Some(prefix) => Some(format!("{prefix}{package_name}@{target}")),
        None if alias_matches => Some(target),
        None => Some(format!("npm:{package_name}@{target}")),
    }
}

/// Resolver-time `trustPolicy='no-downgrade'` check on a fresh pick.
/// No-op unless the policy is `NoDowngrade`. When active, runs
/// [`fail_if_trust_downgraded`] against the picked version using the
/// full packument the picker fetched (forced to full metadata under
/// this policy by the install layer) and propagates a downgrade as a
/// hard [`ResolveError`].
pub(super) fn fail_if_trust_downgraded_for_pick(
    opts: &ResolveOptions,
    picked: &PickedFromRegistry,
    ignore_missing_time_field: bool,
) -> Result<(), ResolveError> {
    if opts.trust_policy != Some(TrustPolicy::NoDowngrade) {
        return Ok(());
    }
    let trust_opts = TrustCheckOptions {
        trust_policy_exclude: opts.trust_policy_exclude.as_ref(),
        trust_policy_ignore_after_minutes: opts.trust_policy_ignore_after,
        now: None,
        ignore_missing_time_field,
    };
    fail_if_trust_downgraded(&picked.meta, &picked.version.version.to_string(), &trust_opts)
        .map_err(|err| Box::new(err) as ResolveError)
}

/// The raw `dist-tags.latest` when the active `minimumReleaseAge`
/// policy would allow installing it, `None` otherwise. The install
/// summary's `(X is available)` hint must only ever name the actual
/// latest tag, so an immature latest suppresses the hint instead of
/// being rewritten to an older mature version. Suppression requires
/// positive evidence of immaturity: a missing or unparsable
/// timestamp keeps the raw tag, matching
/// [`detect_min_release_age_violation`], which likewise only flags a
/// version it can date.
pub(super) fn latest_allowed_by_policy<'a>(
    meta: &'a Package,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Option<&'a str> {
    let latest = meta.dist_tag("latest")?;
    let Some(cutoff) = published_by else { return Some(latest) };
    if let Some(policy) = published_by_exclude {
        use pnpm_config::version_policy::PolicyMatch;
        match policy.matches(&meta.name) {
            PolicyMatch::AnyVersion => return Some(latest),
            PolicyMatch::ExactVersions(versions)
                if versions.iter().any(|exact| exact == latest) =>
            {
                return Some(latest);
            }
            _ => {}
        }
    }
    match meta.published_at(latest).and_then(parse_packument_timestamp) {
        Some(published_at) if published_at > cutoff => None,
        _ => Some(latest),
    }
}

/// Resolver-time `minimumReleaseAge` check. Returns a violation entry
/// when the picked version's publish timestamp falls past the policy
/// cutoff and isn't excluded by name/version.
pub(super) fn detect_min_release_age_violation(
    name: &PkgName,
    version: &str,
    published_at: Option<&str>,
    resolution: &LockfileResolution,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Option<ResolutionPolicyViolation> {
    let cutoff = published_by?;
    let timestamp = published_at?;
    if let Some(policy) = published_by_exclude {
        use pnpm_config::version_policy::PolicyMatch;
        match policy.matches(&name.to_string()) {
            PolicyMatch::AnyVersion => return None,
            PolicyMatch::ExactVersions(versions)
                if versions.iter().any(|exact| exact == version) =>
            {
                return None;
            }
            _ => {}
        }
    }
    let parsed = parse_packument_timestamp(timestamp)?;
    if parsed <= cutoff {
        return None;
    }
    Some(ResolutionPolicyViolation {
        name: name.clone(),
        version: version.to_string(),
        resolution: resolution.clone(),
        code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
        reason: format!(
            "was published at {timestamp}, within the minimumReleaseAge cutoff ({cutoff})",
            cutoff = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ),
    })
}

/// Whether the registry answered "no such package" for this pick.
pub(super) fn is_not_found_error(err: &(dyn std::error::Error + 'static)) -> bool {
    registry_response_status(err) == Some(reqwest::StatusCode::NOT_FOUND)
}

/// The HTTP status a registry answered with, recovered from anywhere in the
/// error chain. Both shapes occur: the picker's raw `reqwest` failure, and the
/// [`RegistryResponseError`] [`map_pick_error`](super::guarded_pick::map_pick_error) restates it as.
pub(super) fn registry_response_status(
    err: &(dyn std::error::Error + 'static),
) -> Option<reqwest::StatusCode> {
    let mut current = Some(err);
    while let Some(err) = current {
        if let Some(response_err) = err.downcast_ref::<RegistryResponseError>() {
            return reqwest::StatusCode::from_u16(response_err.status).ok();
        }
        if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
            && let Some(status) = reqwest_err.status()
        {
            return Some(status);
        }
        current = err.source();
    }
    None
}

/// Keep named-registry and JSR dependencies under their original protocol prefix.
pub(crate) fn prefixed_calculated_specifier(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    prefix: &str,
    name: &str,
    picked: &PackageVersion,
) -> Option<String> {
    revision_specifier(wanted_dependency, opts, spec, Some(prefix), name, &picked.version).or_else(
        || {
            calc_specifier_from(wanted_dependency, opts, spec).map(
                |(bare_specifier, default_pin)| {
                    crate::calc_prefixed_specifier(
                        prefix,
                        name,
                        bare_specifier,
                        wanted_dependency.prev_specifier.as_deref(),
                        wanted_dependency.alias.as_deref(),
                        picked,
                        default_pin,
                    )
                },
            )
        },
    )
}

/// Emit the tarball URL already supplied by the picker, which the install path
/// consumes directly without reconstructing a registry resolution.
pub(super) fn picked_tarball_resolution(
    picked: &PackageVersion,
    registry: &str,
) -> Result<(LockfileResolution, Option<TarballRevision>), ResolveError> {
    let integrity = dist_integrity(&picked.dist)?;
    let revision = tarball_revision(picked, integrity.as_ref(), registry)?;
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: picked.dist.tarball.clone(),
        integrity,
        revision,
        git_hosted: None,
        path: None,
    });
    Ok((resolution, revision))
}

impl BuildResolveResult<'_> {
    pub(super) fn manifest_for_revision(
        &self,
        picked: &PackageVersion,
        version_str: &str,
        revision: Option<TarballRevision>,
    ) -> Result<Arc<serde_json::Value>, ResolveError> {
        cached_manifest(
            self.picked_manifest_cache,
            format!(
                "{}\x00{}@{version_str}+r{}",
                self.registry,
                picked.name,
                revision.map_or(0, TarballRevision::get),
            ),
            picked,
        )
    }
}

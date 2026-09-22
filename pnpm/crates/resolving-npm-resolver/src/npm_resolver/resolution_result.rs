use super::{
    Algorithm, Arc, DateTime, Integrity, InvalidTarballIntegrityError, LockfileResolution, Package,
    PackageDistribution, PackageVersion, PackageVersionPolicy, PickedFromRegistry, PkgName,
    PkgNameVer, PkgResolutionId, RangeSpecStyle, RegistryPackageSpec, RegistryResponseError,
    RegistryRevisionSelector, ResolveError, ResolveOptions, ResolveResult, TarballResolution,
    TarballRevision, TrustCheckOptions, TrustPolicy, Utc, Version, WantedDependency,
    fail_if_trust_downgraded,
    release_policy::{
        detect_min_release_age_violation, installable_under_policy, latest_allowed_by_policy,
    },
    select_package_revision, tarball_revision,
};
use crate::pick_package_from_meta::{
    RegistryPackageSpecType, semver_range::semver_satisfies_loose,
};
use pnpm_resolving_resolver_base::NonDeprecatedAlternative;

/// Inputs used to construct a registry resolution.
pub(crate) struct BuildResolveResult<'a> {
    pub meta: &'a Package,
    pub picked: &'a PackageVersion,
    pub published_by: Option<DateTime<Utc>>,
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
    pub picked_manifest_cache: &'a crate::PickedManifestCache,
    pub blocked_versions: Option<&'a pnpm_resolving_resolver_base::BlockedVersions>,
    pub registry: RegistryResolutionSource<'a>,
    pub specifier: ResolvedSpecifier<'a>,
}

pub(crate) struct RegistryResolutionSource<'a> {
    pub resolved_via: &'a str,
    pub registry: &'a str,
    /// `Some(alias)` when the caller resolves from a named registry and
    /// registry-qualified ids are enabled — the minted id then becomes
    /// `<name>@<alias>:<version>` (lockfile format 12.0).
    pub registry_name: Option<&'a str>,
}

pub(crate) struct ResolvedSpecifier<'a> {
    pub spec: &'a RegistryPackageSpec,
    pub alias: Option<&'a str>,
    /// The manifest-ready specifier for `picked`, rendered in whichever
    /// shape the caller's protocol round-trips through, or `None` when
    /// the caller did not ask for one
    /// (`ResolveOptions::calc_specifier`).
    pub calculated_specifier: Option<String>,
}

pub(crate) fn build_resolve_result(
    args: BuildResolveResult<'_>,
) -> Result<ResolveResult, ResolveError> {
    let picked = select_package_revision(args.picked, args.specifier.spec, args.registry.registry)?;
    let picked = picked.as_ref();
    let pkg_name = PkgName::parse(args.specifier.spec.name.as_str())
        .map_err(|err| Box::new(err) as ResolveError)?;
    let version_str = picked.version.to_string();
    let name_ver = PkgNameVer::new(pkg_name.clone(), picked.version.clone());
    let (resolution, revision) = picked_tarball_resolution(picked, args.registry.registry)?;
    let published_at = args.meta
        .published_at(picked.packument_version.as_deref().unwrap_or(&version_str))
        .map(str::to_string);
    let manifest = args.manifest_for_revision(picked, &version_str, revision)?;
    let id = resolution_id(args.registry.registry_name, &args.specifier.spec.name, picked);
    let policy_violation = detect_min_release_age_violation(
        &pkg_name,
        &version_str,
        published_at.as_deref(),
        &resolution,
        args.published_by,
        args.published_by_exclude,
        args.is_blocked(&version_str),
    );
    let package = resolved_package_info(&args, name_ver, &version_str, published_at, manifest);
    Ok(ResolveResult {
        id,
        policy_violation,
        resolution,
        resolved_via: args.registry.resolved_via.to_string(),
        normalized_bare_specifier: args.specifier.spec.normalized_bare_specifier
            .clone()
            .or(args.specifier.calculated_specifier),
        alias: args.specifier.alias.map(str::to_string),
        package,
    })
}

/// The per-package half of a registry resolution: what a consumer reads off
/// the picked version rather than off the resolution itself.
fn resolved_package_info(
    args: &BuildResolveResult<'_>,
    name_ver: PkgNameVer,
    version_str: &str,
    published_at: Option<String>,
    manifest: Arc<serde_json::Value>,
) -> pnpm_resolving_resolver_base::ResolvedPackageInfo {
    pnpm_resolving_resolver_base::ResolvedPackageInfo {
        requested_name: Some(args.specifier.spec.name.clone()),
        name_ver: Some(name_ver),
        latest: latest_allowed_by_policy(args.meta, args.published_by, args.published_by_exclude)
            .map(str::to_string),
        published_at,
        manifest: Some(manifest),
        non_deprecated_alternative: find_non_deprecated_alternative(
            args.meta,
            version_str,
            args.specifier.spec,
            args.published_by,
            args.published_by_exclude,
        ),
    }
}

pub(super) fn calculated_specifier(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    picked: &PickedFromRegistry,
) -> Option<String> {
    revision_specifier(wanted_dependency, opts, spec, None, &spec.name, &picked.version.version)
        .or_else(|| {
            calc_specifier_from(wanted_dependency, opts, spec)
                .map(|(bare_specifier, default_pin)| {
                    crate::calc_specifier(
                        bare_specifier,
                        wanted_dependency.prev_specifier.as_deref(),
                        wanted_dependency.alias.as_deref(),
                        &picked.version,
                        default_pin,
                    )
                })
        })
}

pub(super) fn resolution_id(
    registry_name: Option<&str>,
    requested_name: &str,
    picked: &PackageVersion,
) -> PkgResolutionId {
    match registry_name {
        Some(registry_name) => {
            PkgResolutionId::from(format!("{requested_name}@{registry_name}:{}", picked.version))
        }
        None => PkgResolutionId::from(format!("{requested_name}@{}", picked.version)),
    }
}

/// Serialize each distinct selected manifest once per install. The key scopes
/// registry, requested name, raw packument key, version, and revision so picks
/// with different artifacts or dependency metadata cannot share a cached value.
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
    let Some(shasum) = dist.shasum
        .as_deref()
        .filter(|shasum| !shasum.is_empty())
    else {
        return Ok(None);
    };
    Integrity::from_hex(shasum, Algorithm::Sha1)
        .map(Some)
        .map_err(|_| {
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
    if !opts.specifier.calc_specifier || spec.normalized_bare_specifier.is_some() {
        return None;
    }
    let bare_specifier = wanted_dependency.bare_specifier.as_deref()?;
    Some((bare_specifier, opts.specifier.range_spec_style.unwrap_or(RangeSpecStyle::Major)))
}

pub(crate) fn revision_specifier(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    prefix: Option<&str>,
    package_name: &str,
    version: &Version,
) -> Option<String> {
    if !opts.specifier.calc_specifier || spec.normalized_bare_specifier.is_some() {
        return None;
    }
    let RegistryRevisionSelector::Valid(revision) = spec.revision.as_ref()? else {
        return None;
    };
    let target = format!("{version}+r{revision}");
    let alias_matches = wanted_dependency.alias
        .as_deref()
        .is_none_or(|alias| alias == package_name);
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
    if opts.policy.trust_policy != Some(TrustPolicy::NoDowngrade) {
        return Ok(());
    }
    let trust_opts = TrustCheckOptions {
        trust_policy_exclude: opts.policy.trust_policy_exclude.as_ref(),
        trust_policy_ignore_after_minutes: opts.policy.trust_policy_ignore_after,
        now: None,
        ignore_missing_time_field,
    };
    fail_if_trust_downgraded(&picked.meta, &picked.version.version.to_string(), &trust_opts)
        .map_err(|err| Box::new(err) as ResolveError)
}

/// The newest version the registry does not report as deprecated, for the
/// deprecation warning to point at.
///
/// `None` unless `picked_version` is itself deprecated, so the scan stays on
/// the rare path. Candidates the active `minimumReleaseAge` policy would
/// refuse are skipped, so the version named is one pnpm would actually
/// install. Read off the packument pnpm already holds, and `is_deprecated`
/// probes a version without hydrating its manifest, which keeps it cheap.
fn find_non_deprecated_alternative(
    meta: &Package,
    picked_version: &str,
    spec: &RegistryPackageSpec,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Option<NonDeprecatedAlternative> {
    if !meta.versions.is_deprecated(picked_version) {
        return None;
    }
    let newest = meta.versions
        .keys()
        .filter(|version| !meta.versions.is_deprecated(version))
        .filter(|version| {
            installable_under_policy(meta, version, published_by, published_by_exclude)
        })
        .filter_map(|version| Version::parse(version).ok())
        .max()?;
    let version = newest.to_string();
    // A tag says nothing about which versions are acceptable, so there is no
    // range for the alternative to fall outside of.
    let outside_declared_range = spec.spec_type == RegistryPackageSpecType::Range
        && spec.fetch_spec != "*"
        && !semver_satisfies_loose(&version, &spec.fetch_spec);
    Some(NonDeprecatedAlternative { version, outside_declared_range })
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
    revision_specifier(wanted_dependency, opts, spec, Some(prefix), name, &picked.version)
        .or_else(|| {
            calc_specifier_from(wanted_dependency, opts, spec)
                .map(|(bare_specifier, default_pin)| {
                    crate::calc_prefixed_specifier(
                        prefix,
                        name,
                        bare_specifier,
                        wanted_dependency.prev_specifier.as_deref(),
                        wanted_dependency.alias.as_deref(),
                        picked,
                        default_pin,
                    )
                })
        })
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
    fn is_blocked(&self, version: &str) -> bool {
        self.blocked_versions
            .and_then(|blocked| blocked.get(&self.specifier.spec.name))
            .is_some_and(|versions| versions.contains(version))
    }

    pub(super) fn manifest_for_revision(
        &self,
        picked: &PackageVersion,
        version_str: &str,
        revision: Option<TarballRevision>,
    ) -> Result<Arc<serde_json::Value>, ResolveError> {
        cached_manifest(
            self.picked_manifest_cache,
            serde_json::to_string(&(
                self.registry.registry,
                &self.specifier.spec.name,
                picked.packument_version.as_deref().unwrap_or(version_str),
                version_str,
                revision.map_or(0, TarballRevision::get),
            ))
            .map_err(|error| Box::new(error) as ResolveError)?,
            picked,
        )
    }
}

impl RegistryResolutionSource<'_> {
    pub(crate) fn build_result(
        self,
        picked: &PickedFromRegistry,
        policy: &pnpm_resolving_resolver_base::ResolutionPolicyOptions,
        picked_manifest_cache: &crate::PickedManifestCache,
        specifier: ResolvedSpecifier<'_>,
    ) -> Result<ResolveResult, ResolveError> {
        build_resolve_result(BuildResolveResult {
            meta: &picked.meta,
            picked: &picked.version,
            blocked_versions: policy.blocked_versions.as_deref(),
            published_by: policy.published_by,
            published_by_exclude: policy.published_by_exclude.as_ref(),
            picked_manifest_cache,
            registry: self,
            specifier,
        })
    }
}

impl<'a> ResolvedSpecifier<'a> {
    /// Preserve the registry protocol and declared package name when saving a picked version.
    pub(crate) fn prefixed(
        wanted: &WantedDependency,
        opts: &ResolveOptions,
        spec: &'a RegistryPackageSpec,
        prefix: &str,
        name: &'a str,
        picked: &PackageVersion,
    ) -> Self {
        Self {
            spec,
            alias: Some(name),
            calculated_specifier: prefixed_calculated_specifier(
                wanted, opts, spec, prefix, name, picked,
            ),
        }
    }
}

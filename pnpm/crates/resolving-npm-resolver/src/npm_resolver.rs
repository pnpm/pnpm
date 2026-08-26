//! npm-registry resolver. Wraps
//! [`parse_bare_specifier`](crate::parse_bare_specifier()) plus
//! [`pick_package`](crate::pick_package()) behind the chain-friendly
//! [`Resolver`] trait so the default-resolver dispatcher can dispatch
//! npm-shaped dependencies through it.
//!
//! The struct owns the registry config + network handles + meta cache;
//! the trait implementation parses the bare specifier, picks a version,
//! and maps the result to [`ResolveResult`].
//!
//! Workspace handling intentionally lives on the npm-resolver side:
//! non-path `workspace:` specs route through
//! [`try_resolve_from_workspace`](crate::try_resolve_from_workspace())
//! to a `link:` / `file:` resolution against the install's workspace
//! package map; the path-relative forms (`workspace:./foo`,
//! `workspace:../bar`) return `Ok(None)` so the local-resolver in the
//! chain claims them.
//!
//! Not yet implemented:
//!
//! - **`peek_manifest_from_store` fast path.** Short-circuiting a
//!   registry fetch when the lockfile-pinned tarball is already in the
//!   store. Pacquet today goes through the picker unconditionally;
//!   adding the fast path is a separate item.

use std::{borrow::Cow, collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use node_semver::Version;
use pnpm_config::{
    DEFAULT_JSR_REGISTRY, NeedsFullMetadataFor, TrustPolicy, version_policy::PackageVersionPolicy,
};
use pnpm_lockfile::{
    LockfileResolution, PkgName, PkgNameVer, TarballResolution, TarballRevision,
    is_integrity_addressed_registry_tarball_url,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient, redact_and_sanitize};
use pnpm_registry::{Package, PackageDistribution, PackageVersion, RangeSpecStyle};
use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, NoMatchingVersionError, PackageVersionGuardDecision, PkgResolutionId,
    RegistryResponseError, RegistryResponseErrorOptions, ResolutionPolicyViolation, ResolveError,
    ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, UpdateBehavior,
    WantedDependency, WorkspacePackages, parse_packument_timestamp,
};
use ssri::{Algorithm, Integrity};

use crate::{
    errors::{
        AllVersionsBlockedError, GuardRepickLimitError, InvalidRevisionSpecifierError,
        InvalidTarballIntegrityError, InvalidTarballRevisionMetadataError,
        MalformedRevisionHistoryError, NoMatchingRevisionError,
    },
    named_registry::pick_registry_for_package,
    parse_bare_specifier::{parse_bare_specifier, parse_jsr_specifier_to_registry_package_spec},
    pick_package::{
        PackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions, pick_package,
    },
    pick_package_from_meta::{
        RegistryPackageSpec, RegistryPackageSpecType, RegistryRevisionSelector,
    },
    registry_url::to_registry_url,
    resolve_from_workspace::{
        ResolveFromWorkspaceError, ResolveFromWorkspaceOptions, SavedSpecifierOptions,
        pick_matching_local_version_or_null, resolve_from_local_package,
        try_resolve_from_workspace, try_resolve_from_workspace_packages,
    },
    trust_checks::{TrustCheckOptions, fail_if_trust_downgraded},
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

/// Provenance tag for [`ResolveResult::resolved_via`] when the picker
/// drove a JSR-prefixed specifier through the `@jsr` registry.
const JSR_REGISTRY_RESOLVED_VIA: &str = "jsr-registry";

/// Provenance tag for npm-registry resolutions.
const NPM_REGISTRY_RESOLVED_VIA: &str = "npm-registry";

/// npm-registry resolver.
///
/// One instance per install. Owns the registries map, named-registry
/// overrides, throttled HTTP client, auth-header table, on-disk
/// metadata mirror root, and the install-shared metadata cache the
/// picker reads through.
pub struct NpmResolver<Cache: PackageMetaCache> {
    /// `default` plus per-scope (`@scope`) entries. The picker consults
    /// the `default` entry as the install-wide default and the scope
    /// entry when the resolved package name carries one. Pacquet today
    /// only populates
    /// `default` — per-scope wiring lands when `.npmrc`'s
    /// `<scope>:registry` parsing does.
    pub registries: HashMap<String, String>,
    /// User-supplied named-registry aliases (e.g. `gh:` →
    /// `https://npm.pkg.github.com/`). Merged with
    /// [`crate::BUILTIN_REGISTRIES_BY_PREFIX`] at construction. Today
    /// only consulted by the named-registry resolver (out of scope
    /// for this port); kept here so the install layer can build one
    /// resolver instance with the full registry view.
    pub registries_by_prefix: HashMap<String, String>,
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    pub meta_cache: Arc<Cache>,
    /// Per-cache-key packument fetch serializer. Shared across this
    /// resolver and the sibling [`crate::NamedRegistryResolver`] so
    /// concurrent picks for the same `(registry, name)` coalesce
    /// into one network fetch. Construct via
    /// [`crate::shared_packument_fetch_locker`] once per install.
    pub fetch_locker: crate::PackumentFetchLocker,
    /// Per-`(pkg_name, version)` cache for the JSON manifest the
    /// resolver builds from the picker output. Shared across this
    /// resolver and [`crate::NamedRegistryResolver`] so picks of the
    /// same package version across registries coalesce. Construct
    /// via [`crate::shared_picked_manifest_cache`] once per install.
    pub picked_manifest_cache: crate::PickedManifestCache,
    /// Root of the on-disk metadata mirror. `None` disables every
    /// disk read/write — the picker goes straight to the network on
    /// each cache miss.
    pub cache_dir: Option<PathBuf>,
    pub offline: bool,
    pub prefer_offline: bool,
    pub ignore_missing_time_field: bool,
    /// Install-wide bias toward full metadata. Threaded through to
    /// [`PickPackageContext::full_metadata`].
    pub full_metadata: bool,
    /// Per-registry answer to the same question, threaded through to
    /// [`PickPackageContext::needs_full_metadata_for`]. Set from
    /// `Config::requires_full_metadata_for_registry` so a registry that
    /// declares `supportsTimeField` is not charged for full metadata.
    pub needs_full_metadata_for: Option<NeedsFullMetadataFor>,
    /// When full metadata is forced, read and write pnpm's filtered
    /// full-metadata mirror.
    pub filter_metadata: bool,
    /// Retry budget threaded through to
    /// [`PickPackageContext::retry_opts`]. Sourced from the install's
    /// `fetch-retries` config.
    pub retry_opts: RetryOpts,
}

impl<Cache: PackageMetaCache + 'static> Resolver for NpmResolver<Cache> {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(self.resolve_impl(wanted_dependency, opts))
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(self.resolve_latest_impl(query, opts))
    }
}

impl<Cache: PackageMetaCache + 'static> NpmResolver<Cache> {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let default_tag = opts.default_tag.as_deref().unwrap_or("latest");

        if let Some(bare) = wanted_dependency.bare_specifier.as_deref()
            && bare.starts_with("workspace:")
        {
            if bare.starts_with("workspace:.") {
                return Ok(None);
            }
            let registry = pick_registry_for_package(
                &self.registries,
                wanted_dependency.alias.as_deref().unwrap_or_default(),
                wanted_dependency.bare_specifier.as_deref(),
            );
            let ws_opts = ResolveFromWorkspaceOptions {
                project_dir: opts.project_dir.as_path(),
                lockfile_dir: opts.lockfile_dir.as_path(),
                registry: &registry,
                default_tag,
                workspace_packages: opts.workspace_packages.as_deref(),
                inject_workspace_packages: opts.inject_workspace_packages,
                saved_specifier: saved_specifier_options(opts),
            };
            return try_resolve_from_workspace(wanted_dependency, &ws_opts)
                .map_err(|err| Box::new(err) as ResolveError);
        }

        // `jsr:` resolves through the `@jsr` registry under the
        // `@jsr/<scope>__<name>` folded name, dispatched alongside the
        // plain npm path.
        if let Some(bare) = wanted_dependency.bare_specifier.as_deref()
            && bare.starts_with("jsr:")
        {
            return self.resolve_jsr_impl(wanted_dependency, opts, bare, default_tag).await;
        }

        // Pick registry from `(alias, bare_specifier)` so an npm-alias
        // entry like `"foo": "npm:@scope/bar@^1"` routes through
        // `registries[@scope]` instead of the alias's own scope.
        let registry = pick_registry_for_package(
            &self.registries,
            wanted_dependency.alias.as_deref().unwrap_or_default(),
            wanted_dependency.bare_specifier.as_deref(),
        );

        let spec = match wanted_dependency.bare_specifier.as_deref() {
            Some(bare) => {
                match parse_bare_specifier(
                    bare,
                    wanted_dependency.alias.as_deref(),
                    default_tag,
                    &registry,
                ) {
                    Some(spec) => spec,
                    None => return Ok(None),
                }
            }
            None => match wanted_dependency.alias.as_deref() {
                Some(alias) if !alias.is_empty() => default_tag_spec(alias, default_tag),
                _ => return Ok(None),
            },
        };
        validate_revision_selector(&spec)?;

        let optional = wanted_dependency.optional.unwrap_or(false);
        let can_keep_workspace_resolution = opts
            .current_pkg
            .as_ref()
            .is_none_or(|current| matches!(current.resolution, LockfileResolution::Directory(_)));
        let workspace_packages_active = (spec.revision.is_none()
            && opts.always_try_workspace_packages
            && (opts.update != UpdateBehavior::Patches || can_keep_workspace_resolution))
            .then_some(opts.workspace_packages.as_ref())
            .flatten();

        // A store-manifest peek, once pacquet grows one, has to run before
        // this fast path — the TypeScript counterpart
        // (`pnpm11/resolving/npm-resolver/src/index.ts`) documents why.
        if opts.prefer_workspace_packages
            && spec.revision.is_none()
            && opts.trust_policy != Some(TrustPolicy::NoDowngrade)
            && !opts.update_checksums
            && !opts.inject_workspace_packages
            && !wanted_dependency.injected.unwrap_or(false)
            && let Some(workspace_packages) = workspace_packages_active
            && let Some(matching_name) = workspace_packages.get(spec.name.as_str())
            && matching_name.len() == 1
            && let Some(local_version) = pick_matching_local_version_or_null(matching_name, &spec)
            && let Some(local_package) = matching_name.get(&local_version)
        {
            return Ok(Some(resolve_from_local_package(
                local_package,
                wanted_dependency,
                false,
                opts.project_dir.as_path(),
                opts.lockfile_dir.as_path(),
                saved_specifier_options(opts),
            )));
        }

        let pick_result = self.pick_from_registry(&registry, &spec, opts, optional).await;
        let picked = match pick_result {
            Ok(RegistryPick::Picked(picked)) => picked,
            Ok(RegistryPick::NoMatchingVersion(meta)) => {
                return match workspace_packages_active.map(|workspace_packages| {
                    try_workspace_fallback(workspace_packages, &spec, wanted_dependency, opts)
                }) {
                    Some(Ok(result)) => Ok(Some(result)),
                    // Neither the registry nor the workspace has a
                    // matching version; the workspace mismatch error
                    // carries the available local versions, which is the
                    // actionable detail here (pnpm/pnpm#1379).
                    Some(Err(
                        ws_err @ ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace {
                            ..
                        },
                    )) => Err(Box::new(ws_err)),
                    _ => Err(no_matching_version(wanted_dependency, &registry, &meta)),
                };
            }
            Err(err) => {
                return match workspace_packages_active.map(|workspace_packages| {
                    try_workspace_fallback(workspace_packages, &spec, wanted_dependency, opts)
                }) {
                    Some(Ok(result)) => Ok(Some(result)),
                    // Surface the workspace mismatch (with its available
                    // versions) only when the registry said "not found";
                    // auth, network, and server errors propagate as-is
                    // (pnpm/pnpm#1379).
                    Some(Err(
                        ws_err @ ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace {
                            ..
                        },
                    )) if is_not_found_error(err.as_ref()) => Err(Box::new(ws_err)),
                    _ => Err(err),
                };
            }
        };

        fail_if_trust_downgraded_for_pick(opts, &picked, self.ignore_missing_time_field)?;

        if spec.revision.is_none()
            && let Some(workspace_packages) = workspace_packages_active
            && let Some(mut result) = try_workspace_shadow(
                workspace_packages,
                &spec,
                &picked.version,
                wanted_dependency,
                opts,
            )
        {
            result.latest = latest_allowed_by_policy(
                &picked.meta,
                opts.published_by,
                opts.published_by_exclude.as_ref(),
            )
            .map(str::to_string);
            return Ok(Some(result));
        }

        let result = build_resolve_result(BuildResolveResult {
            meta: &picked.meta,
            picked: &picked.version,
            spec: &spec,
            alias: wanted_dependency.alias.as_deref(),
            resolved_via: NPM_REGISTRY_RESOLVED_VIA,
            registry: &registry,
            registry_name: None,
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude.as_ref(),
            picked_manifest_cache: &self.picked_manifest_cache,
            calculated_specifier: revision_specifier(
                wanted_dependency,
                opts,
                &spec,
                None,
                &spec.name,
                &picked.version.version,
            )
            .or_else(|| {
                calc_specifier_from(wanted_dependency, opts, &spec).map(
                    |(bare_specifier, default_pin)| {
                        crate::calc_specifier(
                            bare_specifier,
                            wanted_dependency.alias.as_deref(),
                            &picked.version,
                            default_pin,
                        )
                    },
                )
            }),
        })?;

        Ok(Some(result))
    }

    /// JSR counterpart to the npm path: runs the JSR-specifier parser,
    /// picks against the `@jsr` registry, then stamps
    /// `resolved_via = "jsr-registry"` and
    /// `alias = spec.jsr_pkg_name` on the result, so an edge that
    /// declares no name of its own (`pnpm add jsr:@pnpm-e2e/bar`) is
    /// installed under its JSR-style name rather than the folded
    /// `@jsr/…` one. An edge declared under a manifest key keeps that
    /// key.
    async fn resolve_jsr_impl(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        bare_specifier: &str,
        default_tag: &str,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let jsr_spec = parse_jsr_specifier_to_registry_package_spec(
            bare_specifier,
            wanted_dependency.alias.as_deref(),
            default_tag,
        )
        .map_err(|err| Box::new(err) as ResolveError)?;
        let Some(jsr_spec) = jsr_spec else {
            return Ok(None);
        };
        validate_revision_selector(&jsr_spec.spec)?;

        let registry = self.registries.get("@jsr").map_or(DEFAULT_JSR_REGISTRY, String::as_str);

        let optional = wanted_dependency.optional.unwrap_or(false);
        let picked = match self.pick_from_registry(registry, &jsr_spec.spec, opts, optional).await?
        {
            RegistryPick::Picked(picked) => picked,
            RegistryPick::NoMatchingVersion(meta) => {
                return Err(no_matching_version(wanted_dependency, registry, &meta));
            }
        };

        let result = build_resolve_result(BuildResolveResult {
            meta: &picked.meta,
            picked: &picked.version,
            spec: &jsr_spec.spec,
            alias: Some(jsr_spec.jsr_pkg_name.as_str()),
            resolved_via: JSR_REGISTRY_RESOLVED_VIA,
            registry,
            registry_name: None,
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude.as_ref(),
            picked_manifest_cache: &self.picked_manifest_cache,
            // The entry stays a JSR dependency, so it round-trips under
            // the `jsr:` protocol rather than as the npm-shaped range
            // `calc_specifier` would build.
            calculated_specifier: revision_specifier(
                wanted_dependency,
                opts,
                &jsr_spec.spec,
                Some("jsr:"),
                &jsr_spec.jsr_pkg_name,
                &picked.version.version,
            )
            .or_else(|| {
                calc_specifier_from(wanted_dependency, opts, &jsr_spec.spec).map(
                    |(bare_specifier, default_pin)| {
                        crate::calc_prefixed_specifier(
                            "jsr:",
                            &jsr_spec.jsr_pkg_name,
                            bare_specifier,
                            wanted_dependency.alias.as_deref(),
                            &picked.version,
                            default_pin,
                        )
                    },
                )
            }),
        })?;

        Ok(Some(result))
    }

    /// Common picker invocation shared by [`Self::resolve_impl`] and
    /// [`Self::resolve_jsr_impl`].
    async fn pick_from_registry(
        &self,
        registry: &str,
        spec: &RegistryPackageSpec,
        opts: &ResolveOptions,
        optional: bool,
    ) -> Result<RegistryPick, ResolveError> {
        let overlay_selectors =
            crate::preferred_overlay::overlay_merged_selectors(opts, &spec.name);
        let base_selectors =
            overlay_selectors.as_ref().or_else(|| opts.preferred_versions.get(&spec.name));
        let ctx = PickPackageContext {
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            meta_cache: self.meta_cache.as_ref(),
            fetch_locker: &self.fetch_locker,
            cache_dir: self.cache_dir.as_deref(),
            offline: self.offline,
            prefer_offline: self.prefer_offline,
            ignore_missing_time_field: self.ignore_missing_time_field,
            full_metadata: self.full_metadata,
            needs_full_metadata_for: self.needs_full_metadata_for.as_deref(),
            filter_metadata: self.filter_metadata,
            retry_opts: self.retry_opts,
        };

        let picked = pick_from_registry_with_guard(
            &ctx,
            PickFromRegistryOptions {
                registry,
                spec,
                preferred_version_selectors: base_selectors,
                published_by: opts.published_by,
                published_by_exclude: opts.published_by_exclude.as_ref(),
                pick_lowest_version: opts.pick_lowest_version,
                include_latest_tag: opts.update == UpdateBehavior::Latest,
                dry_run: opts.dry_run,
                optional,
                update_checksums: opts.update_checksums || opts.update == UpdateBehavior::Patches,
                trust_policy: opts.trust_policy,
                package_version_guard: opts.package_version_guard.as_ref(),
            },
        )
        .await?;
        if let RegistryPick::Picked(picked) = &picked {
            crate::preferred_overlay::warn_once_on_held_back_update(
                opts,
                spec,
                base_selectors,
                &picked.meta,
                &picked.version.version.to_string(),
            );
        }
        Ok(picked)
    }

    /// Latest-version companion: feed `wanted.bare_specifier` (or
    /// `latest` when missing) plus `update: latest` (or the original
    /// opts under `compatible`) back through `resolve`, then return the
    /// picked manifest.
    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        // Only the `bare_specifier` is rewritten (synthesized to the
        // default tag when missing). Cloning the rest of the wanted
        // dependency preserves `injected` / `prev_specifier` /
        // `optional`, which downstream resolver branches may yet
        // consult even though the npm resolver doesn't today.
        let mut wanted = query.wanted_dependency.clone();
        if wanted.bare_specifier.is_none() {
            wanted.bare_specifier = Some("latest".to_string());
        }
        let mut resolve_opts = opts.clone();
        if !query.compatible {
            resolve_opts.update = UpdateBehavior::Latest;
        }
        let result = match self.resolve_impl(&wanted, &resolve_opts).await {
            Ok(result) => result,
            Err(err) if swallowed_as_no_latest(&err, opts) => {
                return Ok(Some(LatestInfo { latest_manifest: None }));
            }
            Err(err) => return Err(err),
        };
        let Some(result) = result else {
            return Ok(None);
        };
        if result
            .policy_violation
            .as_ref()
            .is_some_and(|violation| violation.code == MINIMUM_RELEASE_AGE_VIOLATION_CODE)
        {
            return Ok(Some(LatestInfo { latest_manifest: None }));
        }
        Ok(Some(LatestInfo { latest_manifest: result.manifest }))
    }
}

/// Whether a latest-version lookup should report "no latest" instead of
/// failing. `minimumReleaseAge` filters the packument before the pick, so
/// every published version can be too young to match — a policy outcome the
/// caller renders as "nothing to update to", not an error.
pub(crate) fn swallowed_as_no_latest(err: &ResolveError, opts: &ResolveOptions) -> bool {
    opts.published_by.is_some() && err.is::<NoMatchingVersionError>()
}

/// The `ERR_PNPM_NO_MATCHING_VERSION` error for a registry that publishes the
/// package but nothing the request accepts.
pub(crate) fn no_matching_version(
    wanted_dependency: &WantedDependency,
    registry: &str,
    meta: &Package,
) -> ResolveError {
    let dep = match wanted_dependency.alias.as_deref() {
        Some(alias) => {
            format!("{alias}@{}", wanted_dependency.bare_specifier.as_deref().unwrap_or_default())
        }
        None => wanted_dependency.bare_specifier.clone().unwrap_or_default(),
    };
    Box::new(NoMatchingVersionError::new(dep, redact_and_sanitize(registry), meta))
}

/// Registry pick was unavailable (no matching version or fetch
/// error); try the workspace as a fallback via
/// [`try_resolve_from_workspace_packages`]. The caller decides which
/// workspace errors to surface and which to swallow in favour of the
/// original registry outcome.
fn try_workspace_fallback(
    workspace_packages: &WorkspacePackages,
    spec: &RegistryPackageSpec,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Result<ResolveResult, ResolveFromWorkspaceError> {
    let ws_opts = workspace_fallback_options(opts);
    try_resolve_from_workspace_packages(workspace_packages, spec, wanted_dependency, &ws_opts)
}

/// Registry pick succeeded; check whether a workspace package
/// shadows it: exact `name@version` match wins; otherwise a higher
/// workspace version wins; otherwise `preferWorkspacePackages` wins.
fn try_workspace_shadow(
    workspace_packages: &WorkspacePackages,
    spec: &RegistryPackageSpec,
    picked: &PackageVersion,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<ResolveResult> {
    let matching_name = workspace_packages.get(picked.name.as_str())?;
    let hard_link = opts.inject_workspace_packages || wanted_dependency.injected.unwrap_or(false);
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_path();

    let picked_version_string = picked.version.to_string();
    if let Some(matched) = matching_name.get(&picked_version_string) {
        return Some(resolve_from_local_package(
            matched,
            wanted_dependency,
            hard_link,
            project_dir,
            lockfile_dir,
            saved_specifier_options(opts),
        ));
    }

    let local_version = pick_matching_local_version_or_null(matching_name, spec)?;
    let local_parsed = Version::parse(&local_version).ok()?;
    let prefer = opts.prefer_workspace_packages || local_parsed > picked.version;
    if !prefer {
        return None;
    }
    let local_package = matching_name.get(&local_version)?;
    Some(resolve_from_local_package(
        local_package,
        wanted_dependency,
        hard_link,
        project_dir,
        lockfile_dir,
        saved_specifier_options(opts),
    ))
}

/// Build the [`ResolveFromWorkspaceOptions`] bag the workspace
/// fallback helper expects. `registry` and `default_tag` are unused on
/// the fallback path (the spec has already been parsed against the
/// registry) so dummy values are passed through.
fn workspace_fallback_options(opts: &ResolveOptions) -> ResolveFromWorkspaceOptions<'_> {
    const UNUSED: &str = "";
    ResolveFromWorkspaceOptions {
        project_dir: opts.project_dir.as_path(),
        lockfile_dir: opts.lockfile_dir.as_path(),
        registry: UNUSED,
        default_tag: UNUSED,
        workspace_packages: opts.workspace_packages.as_deref(),
        inject_workspace_packages: opts.inject_workspace_packages,
        saved_specifier: saved_specifier_options(opts),
    }
}

/// Project the specifier-writing knobs out of [`ResolveOptions`] for the
/// workspace entry point, which carries its own options struct.
fn saved_specifier_options(opts: &ResolveOptions) -> SavedSpecifierOptions {
    SavedSpecifierOptions {
        calc_specifier: opts.calc_specifier,
        range_spec_style: opts.range_spec_style,
        save_workspace_protocol: opts.save_workspace_protocol,
    }
}

/// `bare_specifier` is absent but `alias` is present: synthesize a tag
/// spec pointing at the default tag.
fn default_tag_spec(alias: &str, default_tag: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: alias.to_string(),
        fetch_spec: default_tag.to_string(),
        spec_type: RegistryPackageSpecType::Tag,
        revision: None,
        normalized_bare_specifier: None,
    }
}

/// Picker output threaded through to [`build_resolve_result`].
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
    /// [`NoMatchingVersionError`] reports what *is* published.
    NoMatchingVersion(std::sync::Arc<Package>),
}

pub(crate) struct PickFromRegistryOptions<'a> {
    pub registry: &'a str,
    pub spec: &'a RegistryPackageSpec,
    pub preferred_version_selectors: Option<&'a pnpm_resolving_resolver_base::VersionSelectors>,
    pub published_by: Option<DateTime<Utc>>,
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
    pub pick_lowest_version: bool,
    pub include_latest_tag: bool,
    pub dry_run: bool,
    pub optional: bool,
    pub update_checksums: bool,
    pub trust_policy: Option<TrustPolicy>,
    pub package_version_guard:
        Option<&'a Arc<dyn pnpm_resolving_resolver_base::PackageVersionGuard>>,
}

/// Upper bound on guard rejections for one package before the resolver
/// gives up. Far beyond any realistic run of consecutive blocked
/// versions, so it only fires on a pathological/hostile packument.
const GUARD_REPICK_LIMIT: usize = 1000;

pub(crate) async fn pick_from_registry_with_guard<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    opts: PickFromRegistryOptions<'_>,
) -> Result<RegistryPick, ResolveError> {
    let mut blocked_versions = std::collections::HashSet::new();
    let mut last_rejection: Option<String> = None;
    loop {
        let pick_opts = PickPackageOptions {
            registry: opts.registry,
            preferred_version_selectors: opts.preferred_version_selectors,
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude,
            pick_lowest_version: opts.pick_lowest_version,
            include_latest_tag: opts.include_latest_tag,
            dry_run: opts.dry_run,
            optional: opts.optional,
            update_checksums: opts.update_checksums,
            trust_policy: opts.trust_policy,
            blocked_versions: (!blocked_versions.is_empty()).then_some(&blocked_versions),
        };
        let pick_result = pick_package(ctx, opts.spec, &pick_opts)
            .await
            .map_err(|err| map_pick_error(ctx, &opts, err))?;

        let Some(version) = pick_result.picked_package else {
            // No candidate left. With no prior guard rejection this is the
            // ordinary "no matching version" outcome; once the guard has
            // rejected every match, surface that as a distinct error rather
            // than blaming the range the user wrote.
            return match last_rejection {
                Some(reason) => Err(all_versions_blocked(opts.spec, reason)),
                None => Ok(RegistryPick::NoMatchingVersion(pick_result.meta)),
            };
        };
        let Some(guard) = opts.package_version_guard else {
            return Ok(RegistryPick::Picked(PickedFromRegistry {
                meta: pick_result.meta,
                version,
            }));
        };

        let version_str = version.version.to_string();
        match guard.check(&opts.spec.name, &version_str).await? {
            PackageVersionGuardDecision::Allow => {
                return Ok(RegistryPick::Picked(PickedFromRegistry {
                    meta: pick_result.meta,
                    version,
                }));
            }
            PackageVersionGuardDecision::Reject { reason } => {
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver",
                    name = %opts.spec.name,
                    version = %version_str,
                    reason = %reason,
                    "package version rejected by resolver guard",
                );
                // Block by the *packument key*, which the next pick filters
                // on. It usually equals the parsed manifest version, but a
                // registry that serves a key differing from the manifest's
                // `version` field would otherwise never get the candidate
                // excluded — re-selecting it forever and wrongly reporting
                // every version blocked when a lower one is still fine.
                let blocked_key = blocked_packument_key(&pick_result.meta, &version, &version_str);
                // A `false` return means the picker re-selected a key we
                // already blocked, so it can't be excluded; stop rather than
                // loop forever — every match really is blocked.
                if !blocked_versions.insert(blocked_key) {
                    return Err(all_versions_blocked(opts.spec, reason));
                }
                // Each rejection re-runs the picker over the packument, so an
                // unbounded run is O(versions²). Cap it well above any real
                // run of consecutive rejected versions to bound the work a
                // hostile packument can force. This is a safety cutoff, not
                // proof every version is blocked, so report it as its own
                // error rather than "all versions blocked".
                if blocked_versions.len() >= GUARD_REPICK_LIMIT {
                    return Err(Box::new(GuardRepickLimitError {
                        name: opts.spec.name.clone(),
                        limit: GUARD_REPICK_LIMIT,
                        reason,
                    }));
                }
                last_rejection = Some(reason);
            }
        }
    }
}

/// The packument key for a picked version, so the guard loop can block the
/// exact entry the next pick filters on. Fast-paths the common case where
/// the parsed manifest version is itself the key; only falls back to
/// locating the key by identity when a registry served a mismatched key.
fn blocked_packument_key(
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

fn all_versions_blocked(spec: &RegistryPackageSpec, reason: String) -> ResolveError {
    Box::new(AllVersionsBlockedError { name: spec.name.clone(), reason })
}

/// Box a picker failure for the resolver chain, restating a non-2xx registry
/// answer as [`RegistryResponseError`]. Without that the transport-level
/// message ("HTTP status client error (404 Not Found) for url ...") is all the
/// user sees: no `ERR_PNPM_FETCH_404`, and no hint that a 404 from a private
/// registry is often really an authorization failure.
fn map_pick_error<Cache: PackageMetaCache>(
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
    let auth_header_value = ctx.auth_headers.for_url_with_package(&url, Some(&opts.spec.name));
    Box::new(RegistryResponseError::new(RegistryResponseErrorOptions {
        url: &redact_and_sanitize(&url),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default(),
        pkg_name: &opts.spec.name,
        auth_header_value: auth_header_value.as_deref(),
    }))
}

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
    let BuildResolveResult {
        meta,
        picked,
        spec,
        alias,
        resolved_via,
        registry,
        registry_name,
        published_by,
        published_by_exclude,
        picked_manifest_cache,
        calculated_specifier,
    } = args;
    let picked = select_package_revision(picked, spec, registry)?;
    let picked = picked.as_ref();
    let pkg_name =
        PkgName::parse(picked.name.as_str()).map_err(|err| Box::new(err) as ResolveError)?;
    let version_str = picked.version.to_string();
    let name_ver = PkgNameVer::new(pkg_name.clone(), picked.version.clone());
    let id = match registry_name {
        Some(registry_name) => {
            PkgResolutionId::from(format!("{}@{registry_name}:{}", picked.name, picked.version))
        }
        None => (&name_ver).into(),
    };
    // The picker always carries a tarball URL on its `dist` payload —
    // every npm registry serves `dist.tarball` on a successful pick
    // and pacquet's deserializer requires it (`dist.tarball: String`,
    // not `Option`). Always emit `Tarball`, never `Registry`. The
    // install side's `extract_tarball` only handles `Tarball`, so
    // mixing the two shapes would force a Registry → URL
    // reconstruction with no payoff: at resolve time we already have
    // the URL the install path needs.
    let integrity = dist_integrity(&picked.dist)?;
    let revision = picked
        .dist
        .revision
        .as_ref()
        .map(|revision| {
            revision
                .as_u64()
                .ok_or_else(|| "the revision is not a positive safe integer".to_string())
                .and_then(|revision| {
                    TarballRevision::try_from(revision).map_err(|error| error.to_string())
                })
        })
        .transpose()
        .map_err(|reason| {
            Box::new(InvalidTarballRevisionMetadataError::new(&picked.dist.tarball, reason))
                as ResolveError
        })?;
    if revision.is_some()
        && !integrity.as_ref().is_some_and(|integrity| {
            is_integrity_addressed_registry_tarball_url(&picked.dist.tarball, integrity, registry)
        })
    {
        return Err(Box::new(InvalidTarballRevisionMetadataError::new(
            &picked.dist.tarball,
            "the URL does not match its complete sha512 integrity and registry",
        )));
    }
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: picked.dist.tarball.clone(),
        integrity,
        revision,
        git_hosted: None,
        path: None,
    });
    let published_at = meta.published_at(&version_str).map(str::to_string);
    // Dedupe `serde_json::to_value(picked)` across picks of the
    // same `(registry, pkg_name, version)` triple — see
    // [`PickedManifestCache`] for the rationale. The cache is shared
    // across the npm / JSR / named-registry resolvers, so the key
    // has to scope by `registry` too; two registries may serve
    // different artifacts under the same `name@version`, and
    // collapsing them would hand the second registry's resolver
    // the first registry's manifest — wrong dependency graph,
    // wrong peers, wrong lockfile metadata. Matches `meta_cache`'s
    // `{registry}\x00{name}` scoping shape.
    let manifest_cache_key = format!(
        "{registry}\x00{}@{version_str}+r{}",
        picked.name,
        revision.map_or(0, TarballRevision::get),
    );
    let manifest = if let Some(cached) = picked_manifest_cache.get(&manifest_cache_key) {
        Some(Arc::clone(cached.value()))
    } else {
        let arc =
            Arc::new(serde_json::to_value(picked).map_err(|err| Box::new(err) as ResolveError)?);
        picked_manifest_cache.insert(manifest_cache_key, Arc::clone(&arc));
        Some(arc)
    };
    let policy_violation = detect_min_release_age_violation(
        &pkg_name,
        &version_str,
        published_at.as_deref(),
        &resolution,
        published_by,
        published_by_exclude,
    );
    Ok(ResolveResult {
        id,
        name_ver: Some(name_ver),
        latest: latest_allowed_by_policy(meta, published_by, published_by_exclude)
            .map(str::to_string),
        published_at,
        manifest,
        resolution,
        resolved_via: resolved_via.to_string(),
        normalized_bare_specifier: spec.normalized_bare_specifier.clone().or(calculated_specifier),
        alias: alias.map(str::to_string),
        policy_violation,
    })
}

fn select_package_revision<'a>(
    picked: &'a PackageVersion,
    spec: &RegistryPackageSpec,
    registry: &str,
) -> Result<Cow<'a, PackageVersion>, ResolveError> {
    validate_current_package_revision(picked, registry)?;
    let Some(selector) = spec.revision.as_ref() else {
        return Ok(Cow::Borrowed(picked));
    };
    let requested = match selector {
        RegistryRevisionSelector::Valid(revision) => *revision,
        RegistryRevisionSelector::Invalid(specifier) => {
            return Err(Box::new(InvalidRevisionSpecifierError { specifier: specifier.clone() }));
        }
    };
    let version = picked.version.to_string();
    if picked.dist.revisions.is_none() {
        if requested == 0 && picked.dist.revision.is_none() {
            return Ok(Cow::Borrowed(picked));
        }
        return Err(Box::new(NoMatchingRevisionError {
            name: picked.name.clone(),
            version,
            revision: requested,
        }));
    }
    let Some(record) = package_revision_record(picked, requested, registry)? else {
        return Err(Box::new(NoMatchingRevisionError {
            name: picked.name.clone(),
            version,
            revision: requested,
        }));
    };

    let mut selected =
        serde_json::to_value(picked).map_err(|error| Box::new(error) as ResolveError)?;
    let selected_object = selected.as_object_mut().expect("PackageVersion serializes as an object");
    const REVISION_MANIFEST_FIELDS: [&str; 12] = [
        "dependencies",
        "optionalDependencies",
        "peerDependencies",
        "peerDependenciesMeta",
        "bundledDependencies",
        "bundleDependencies",
        "bin",
        "engines",
        "os",
        "cpu",
        "libc",
        "hasInstallScript",
    ];
    for field in REVISION_MANIFEST_FIELDS {
        selected_object.remove(field);
    }
    for field in REVISION_MANIFEST_FIELDS {
        if let Some(value) = record.manifest.get(field) {
            selected_object.insert(field.to_string(), value.clone());
        }
    }
    let dist = selected_object
        .get_mut("dist")
        .and_then(serde_json::Value::as_object_mut)
        .expect("PackageVersion.dist serializes as an object");
    dist.insert(
        "integrity".to_string(),
        serde_json::Value::String(record.integrity_text.to_string()),
    );
    dist.insert("tarball".to_string(), serde_json::Value::String(record.tarball.to_string()));
    dist.remove("shasum");
    if requested == 0 {
        dist.remove("revision");
    } else {
        dist.insert("revision".to_string(), serde_json::Value::Number(requested.into()));
    }
    serde_json::from_value(selected)
        .map(Cow::Owned)
        .map_err(|error| malformed_revision_history(picked, error.to_string()))
}

pub(crate) fn validate_revision_selector(spec: &RegistryPackageSpec) -> Result<(), ResolveError> {
    let Some(RegistryRevisionSelector::Invalid(specifier)) = spec.revision.as_ref() else {
        return Ok(());
    };
    Err(Box::new(InvalidRevisionSpecifierError { specifier: specifier.clone() }))
}

struct ValidatedPackageRevision<'a> {
    integrity: Integrity,
    integrity_text: &'a str,
    tarball: &'a str,
    manifest: &'a serde_json::Map<String, serde_json::Value>,
}

fn package_revision_record<'a>(
    picked: &'a PackageVersion,
    requested: u64,
    registry: &str,
) -> Result<Option<ValidatedPackageRevision<'a>>, ResolveError> {
    let Some(revisions) = picked.dist.revisions.as_ref() else { return Ok(None) };
    let Some(revisions) = revisions.as_array() else {
        return Err(malformed_revision_history(picked, "the revisions field is not an array"));
    };
    let matches: Vec<&serde_json::Value> = revisions
        .iter()
        .filter(|entry| {
            entry.get("revision").and_then(serde_json::Value::as_u64) == Some(requested)
        })
        .collect();
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() != 1 {
        return Err(malformed_revision_history(
            picked,
            format!("revision {requested} is advertised more than once"),
        ));
    }
    if requested > pnpm_lockfile::MAX_TARBALL_REVISION {
        return Err(malformed_revision_history(
            picked,
            "a revision is not a canonical safe integer",
        ));
    }
    let record = matches[0];
    let integrity_text =
        record.get("integrity").and_then(serde_json::Value::as_str).ok_or_else(|| {
            malformed_revision_history(picked, format!("revision {requested} has no integrity"))
        })?;
    let integrity = integrity_text.parse::<Integrity>().map_err(|_| {
        malformed_revision_history(picked, format!("revision {requested} has invalid integrity"))
    })?;
    let tarball = record.get("tarball").and_then(serde_json::Value::as_str).ok_or_else(|| {
        malformed_revision_history(picked, format!("revision {requested} has no tarball URL"))
    })?;
    if !is_integrity_addressed_registry_tarball_url(tarball, &integrity, registry) {
        return Err(malformed_revision_history(
            picked,
            format!("revision {requested} is not addressed by its complete sha512 integrity"),
        ));
    }
    let manifest =
        record.get("manifest").and_then(serde_json::Value::as_object).ok_or_else(|| {
            malformed_revision_history(
                picked,
                format!("revision {requested} has an invalid manifest"),
            )
        })?;
    Ok(Some(ValidatedPackageRevision { integrity, integrity_text, tarball, manifest }))
}

fn validate_current_package_revision(
    picked: &PackageVersion,
    registry: &str,
) -> Result<(), ResolveError> {
    let Some(raw_revision) = picked.dist.revision.as_ref() else { return Ok(()) };
    let revision = raw_revision
        .as_u64()
        .and_then(|revision| TarballRevision::try_from(revision).ok())
        .map(TarballRevision::get)
        .ok_or_else(|| {
            malformed_revision_history(
                picked,
                format!("current revision {raw_revision} is not a canonical positive safe integer"),
            )
        })?;
    let record = package_revision_record(picked, revision, registry)?.ok_or_else(|| {
        malformed_revision_history(
            picked,
            format!("current revision {revision} has no history entry"),
        )
    })?;
    if picked.dist.integrity.as_ref() != Some(&record.integrity)
        || !same_registry_artifact_url(&picked.dist.tarball, record.tarball)
    {
        return Err(malformed_revision_history(
            picked,
            format!("revision {revision} does not match the current artifact"),
        ));
    }
    Ok(())
}

fn same_registry_artifact_url(left: &str, right: &str) -> bool {
    match (reqwest::Url::parse(left), reqwest::Url::parse(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn malformed_revision_history(picked: &PackageVersion, reason: impl Into<String>) -> ResolveError {
    Box::new(MalformedRevisionHistoryError {
        name: picked.name.clone(),
        version: picked.version.to_string(),
        reason: reason.into(),
    })
}

/// The integrity a registry version's `dist` pins its tarball with.
///
/// A registry predating subresource integrity publishes only the legacy
/// `dist.shasum` hex digest, which pins the bytes just as well, so it is
/// promoted to its `sha1-` SRI form. `None` when the version pins
/// nothing at all.
fn dist_integrity(dist: &PackageDistribution) -> Result<Option<Integrity>, ResolveError> {
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
fn fail_if_trust_downgraded_for_pick(
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
fn latest_allowed_by_policy<'a>(
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
fn detect_min_release_age_violation(
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
fn is_not_found_error(err: &(dyn std::error::Error + 'static)) -> bool {
    registry_response_status(err) == Some(reqwest::StatusCode::NOT_FOUND)
}

/// The HTTP status a registry answered with, recovered from anywhere in the
/// error chain. Both shapes occur: the picker's raw `reqwest` failure, and the
/// [`RegistryResponseError`] [`map_pick_error`] restates it as.
fn registry_response_status(
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

#[cfg(test)]
mod tests;

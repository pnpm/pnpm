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
pub use resolution_result::normalize_tarball_url;
pub(crate) use resolution_result::{RegistryResolutionSource, ResolvedSpecifier};

pub(crate) use package_revision::validate_revision_selector;

pub(crate) use guarded_pick::{
    CandidateChecks, PickFromRegistryOptions, PickedFromRegistry, RegistryPick,
    pick_from_registry_with_guard,
};

pub(crate) use workspace_pick::{no_matching_version, swallowed_as_no_latest};

mod release_policy;
mod resolution_result;
use release_policy::latest_allowed_by_policy;
use resolution_result::{
    calculated_specifier, is_not_found_error, registry_response_status, trust_check_for_pick,
};

mod package_revision;
use package_revision::{select_package_revision, tarball_revision};

mod guarded_pick;

mod workspace_pick;
use workspace_pick::{
    resolve_workspace_protocol, wanted_spec, workspace_fallback_for, workspace_packages_active,
    workspace_shadow_pick,
};

mod store_peek;
use store_peek::fast_path_pick;

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
    GuardExhaustionPolicy, LatestInfo, LatestQuery, NoMatchingVersionError,
    PackageVersionGuardDecision, PkgResolutionId, RegistryResponseError,
    RegistryResponseErrorOptions, ResolutionPolicyViolation, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, UpdateBehavior, WantedDependency,
    WorkspacePackages, parse_packument_timestamp,
};
use pnpm_store_dir::SharedReadonlyStoreIndex;
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
    trust_checks::TrustCheckOptions,
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
    pub metadata: RegistryMetadataClient<Cache>,
    pub format: RegistryMetadataFormat,
    pub cache_policy: crate::MetadataCachePolicy,
    pub store_index: Option<SharedReadonlyStoreIndex>,
}

pub struct RegistryMetadataClient<Cache: PackageMetaCache> {
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
    /// Retry budget threaded through to
    /// [`crate::MetadataHttpClient::retry_opts`]. Sourced from the install's
    /// `fetch-retries` config.
    pub retry_opts: RetryOpts,
}

pub struct RegistryMetadataFormat {
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
        let default_tag = opts.version.default_tag.as_deref().unwrap_or("latest");

        if let Some(bare) = wanted_dependency.bare_specifier.as_deref()
            && bare.starts_with("workspace:")
        {
            return resolve_workspace_protocol(
                &self.registries,
                wanted_dependency,
                opts,
                bare,
                default_tag,
            );
        }

        // `jsr:` resolves through the `@jsr` registry under the
        // `@jsr/<scope>__<name>` folded name, dispatched alongside the
        // plain npm path.
        if let Some(bare) = wanted_dependency.bare_specifier.as_deref()
            && bare.starts_with("jsr:")
        {
            return self.resolve_jsr_impl(wanted_dependency, opts, bare, default_tag).await;
        }

        self.resolve_registry_dependency(wanted_dependency, opts, default_tag).await
    }

    async fn resolve_registry_dependency(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        default_tag: &str,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some((registry, spec)) = self.prepare_registry_spec(wanted_dependency, default_tag)?
        else {
            return Ok(None);
        };

        let optional = wanted_dependency.optional.unwrap_or(false);
        let workspace_packages_active = workspace_packages_active(opts, &spec);

        if let Some(result) = fast_path_pick(
            self.store_index.as_ref(),
            wanted_dependency,
            opts,
            &spec,
            workspace_packages_active,
        )
        .await?
        {
            return Ok(Some(result));
        }

        let trust_check = trust_check_for_pick(opts, self.cache_policy.ignore_missing_time_field);
        let picked =
            match self.pick_from_registry(&registry, &spec, opts, optional, trust_check).await {
                Ok(RegistryPick::Picked(picked)) => picked,
                outcome => {
                    return workspace_fallback_for(
                        outcome,
                        wanted_dependency,
                        &registry,
                        workspace_packages_active,
                        &spec,
                        opts,
                    );
                }
            };

        self.finish_registry_pick(
            wanted_dependency,
            opts,
            &spec,
            &registry,
            &picked,
            workspace_packages_active,
        )
    }

    fn prepare_registry_spec(
        &self,
        wanted_dependency: &WantedDependency,
        default_tag: &str,
    ) -> Result<Option<(String, RegistryPackageSpec)>, ResolveError> {
        let registry = pick_registry_for_package(
            &self.registries,
            wanted_dependency.alias.as_deref().unwrap_or_default(),
            wanted_dependency.bare_specifier.as_deref(),
        );
        let Some(spec) = wanted_spec(wanted_dependency, default_tag, &registry) else {
            return Ok(None);
        };
        validate_revision_selector(&spec)?;
        Ok(Some((registry, spec)))
    }

    fn finish_registry_pick(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        spec: &RegistryPackageSpec,
        registry: &str,
        picked: &PickedFromRegistry,
        workspace_packages_active: Option<&Arc<pnpm_resolving_resolver_base::WorkspacePackages>>,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        if let Some(result) =
            workspace_shadow_pick(workspace_packages_active, spec, picked, wanted_dependency, opts)
        {
            return Ok(Some(result));
        }

        self.registry_pick_result(wanted_dependency, opts, spec, registry, picked)
    }

    fn registry_pick_result(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        spec: &RegistryPackageSpec,
        registry: &str,
        picked: &PickedFromRegistry,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        crate::npm_resolver::RegistryResolutionSource {
            resolved_via: NPM_REGISTRY_RESOLVED_VIA,
            registry,
            registry_name: None,
        }
        .build_result(
            picked,
            &opts.policy,
            &self.metadata.picked_manifest_cache,
            crate::npm_resolver::ResolvedSpecifier {
                spec,
                alias: wanted_dependency.alias.as_deref(),
                calculated_specifier: calculated_specifier(wanted_dependency, opts, spec, picked),
            },
        )
        .map(Some)
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
        let picked =
            match self.pick_from_registry(registry, &jsr_spec.spec, opts, optional, None).await? {
                RegistryPick::Picked(picked) => picked,
                RegistryPick::NoMatchingVersion(meta) => {
                    return Err(no_matching_version(wanted_dependency, registry, &meta));
                }
            };

        crate::npm_resolver::RegistryResolutionSource {
            resolved_via: JSR_REGISTRY_RESOLVED_VIA,
            registry,
            registry_name: None,
        }
        .build_result(
            &picked,
            &opts.policy,
            &self.metadata.picked_manifest_cache,
            crate::npm_resolver::ResolvedSpecifier::prefixed(
                wanted_dependency,
                opts,
                &jsr_spec.spec,
                "jsr:",
                &jsr_spec.jsr_pkg_name,
                &picked.version,
            ),
        )
        .map(Some)
    }

    async fn pick_from_registry(
        &self,
        registry: &str,
        spec: &RegistryPackageSpec,
        opts: &ResolveOptions,
        optional: bool,
        trust_check: Option<TrustCheckOptions<'_>>,
    ) -> Result<RegistryPick, ResolveError> {
        let overlay_selectors =
            crate::preferred_overlay::overlay_merged_selectors(opts, &spec.name);
        let base_selectors = overlay_selectors
            .as_ref()
            .or_else(|| opts.version.preferred_versions.get(&spec.name));
        let ctx = self.metadata.pick_context(&self.format, self.cache_policy);

        let picked = pick_from_registry_with_guard(
            &ctx,
            PickFromRegistryOptions {
                registry,
                spec,
                preferred_version_selectors: base_selectors,
                pick_lowest_version: opts.version.pick_lowest_version,
                include_latest_tag: opts.refresh.update == UpdateBehavior::Latest,
                checks: crate::npm_resolver::CandidateChecks::new(&opts.policy, trust_check),
                policy: crate::PackagePickPolicy {
                    published_by: opts.policy.published_by,
                    published_by_exclude: opts.policy.published_by_exclude.as_ref(),
                    trust_policy: opts.policy.trust_policy,
                },
                request: crate::MetadataPickRequest {
                    dry_run: opts.refresh.dry_run,
                    optional,
                    update_checksums: opts.refresh.update_checksums
                        || opts.refresh.update == UpdateBehavior::Patches,
                },
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
            resolve_opts.refresh.update = UpdateBehavior::Latest;
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
        if result.policy_violation
            .as_ref()
            .is_some_and(|violation| violation.code == MINIMUM_RELEASE_AGE_VIOLATION_CODE)
        {
            return Ok(Some(LatestInfo { latest_manifest: None }));
        }
        Ok(Some(LatestInfo { latest_manifest: result.package.manifest }))
    }
}

#[cfg(test)]
mod tests;

impl<Cache: PackageMetaCache> RegistryMetadataClient<Cache> {
    pub(crate) fn pick_context<'a>(
        &'a self,
        format: &'a RegistryMetadataFormat,
        cache_policy: crate::MetadataCachePolicy,
    ) -> PickPackageContext<'a, Cache> {
        PickPackageContext {
            full_metadata: format.full_metadata,
            needs_full_metadata_for: format.needs_full_metadata_for.as_deref(),
            filter_metadata: format.filter_metadata,
            cache_policy,
            metadata: crate::MetadataRequestContext {
                meta_cache: self.meta_cache.as_ref(),
                fetch_locker: &self.fetch_locker,
                cache_dir: self.cache_dir.as_deref(),
                http: crate::MetadataHttpClient {
                    http_client: &self.http_client,
                    auth_headers: &self.auth_headers,
                    retry_opts: self.retry_opts,
                },
            },
        }
    }
}

//! Named-registry resolver.
//!
//! Ports upstream's
//! [`resolveFromNamedRegistry`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L674-L704):
//! parses a `<alias>:` specifier through
//! [`parse_named_registry_specifier_to_registry_package_spec`], looks
//! the alias up in the merged named-registries map, and picks the
//! version against that registry's URL. The result carries
//! `resolved_via = "named-registry"` and the scoped package name as
//! the alias so the install layer records the dependency under its
//! original name.
//!
//! Authentication piggybacks on the existing per-URL `.npmrc`
//! mechanism: a `//npm.pkg.github.com/:_authToken=...` entry takes
//! effect for `gh:` specifiers (and analogously for any user-configured
//! alias) because the resolver looks the auth header up by the
//! resolved registry URL, not the alias name.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, UpdateBehavior, WantedDependency,
};

use crate::{
    npm_resolver::{BuildResolveResult, PickedFromRegistry, build_resolve_result},
    parse_bare_specifier::{
        NamedRegistryPackageSpec, parse_named_registry_specifier_to_registry_package_spec,
    },
    pick_package::{PackageMetaCache, PickPackageContext, PickPackageOptions, pick_package},
    pick_package_from_meta::RegistryPackageSpec,
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

/// Provenance tag emitted on [`ResolveResult::resolved_via`] when the
/// picker drove a `<alias>:` specifier through a configured named
/// registry. Mirrors upstream's
/// [`resolveFromNamedRegistry`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L697).
const NAMED_REGISTRY_RESOLVED_VIA: &str = "named-registry";

/// Named-registry resolver.
///
/// One instance per install. Mirrors upstream's named-registry shell
/// of [`createNpmResolver`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L192-L289)
/// — same plumbing as [`crate::NpmResolver`], with the
/// already-merged-and-validated map of named registries and a
/// precomputed set of alias names the parser checks against.
///
/// Construct the maps with
/// [`crate::merge_named_registries`] so user-defined URLs are
/// validated up front and the built-in `gh:` alias is always
/// present.
pub struct NamedRegistryResolver<Cache: PackageMetaCache> {
    /// Merged map of `<alias> → <registry URL>`. Built-in entries
    /// (`gh:` → GitHub Packages) plus any user-supplied overrides
    /// from `pnpm-workspace.yaml#namedRegistries`. Already validated
    /// — every URL parses and is http(s).
    pub named_registries: HashMap<String, String>,
    /// Precomputed key set of [`Self::named_registries`]. The parser
    /// checks aliases against this set per call, so caching it
    /// avoids rebuilding the set for every resolve.
    pub registry_names: HashSet<String>,
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    pub meta_cache: Arc<Cache>,
    /// Shared per-cache-key packument fetch serializer. See
    /// [`crate::PackumentFetchLocker`]. Same handle as the sibling
    /// [`crate::NpmResolver`] so concurrent picks for the same
    /// `(registry, name)` across resolvers coalesce.
    pub fetch_locker: crate::PackumentFetchLocker,
    /// Shared per-`(pkg_name, version)` manifest JSON cache. See
    /// [`crate::PickedManifestCache`]. Same handle as the sibling
    /// [`crate::NpmResolver`].
    pub picked_manifest_cache: crate::PickedManifestCache,
    pub cache_dir: Option<PathBuf>,
    pub offline: bool,
    pub prefer_offline: bool,
    pub ignore_missing_time_field: bool,
    /// Install-wide bias toward full metadata. Threaded through to
    /// [`PickPackageContext::full_metadata`]. Mirrors upstream's
    /// [`ctx.fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L175).
    pub full_metadata: bool,
    /// Retry budget threaded through to
    /// [`PickPackageContext::retry_opts`]. Same `fetch-retries`-sourced
    /// budget the sibling [`crate::NpmResolver`] uses.
    pub retry_opts: RetryOpts,
}

impl<Cache: PackageMetaCache + 'static> Resolver for NamedRegistryResolver<Cache> {
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

impl<Cache: PackageMetaCache + 'static> NamedRegistryResolver<Cache> {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(bare_specifier) = wanted_dependency.bare_specifier.as_deref() else {
            return Ok(None);
        };
        let default_tag = opts.default_tag.as_deref().unwrap_or("latest");

        let parsed = parse_named_registry_specifier_to_registry_package_spec(
            bare_specifier,
            &self.registry_names,
            wanted_dependency.alias.as_deref(),
            default_tag,
        )
        .map_err(|err| Box::new(err) as ResolveError)?;
        let Some(NamedRegistryPackageSpec { spec, registry_name }) = parsed else {
            return Ok(None);
        };

        // Defensive: should never trigger because the parser checks
        // the alias set first, but match upstream's belt-and-braces
        // guard at
        // <https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L689-L690>.
        let Some(registry) = self.named_registries.get(&registry_name) else {
            return Ok(None);
        };

        let optional = wanted_dependency.optional.unwrap_or(false);
        let Some(picked) = self.pick_from_registry(registry, &spec, opts, optional).await? else {
            return Ok(None);
        };

        // Mirror upstream: the dependency is recorded under the
        // scoped package name the named registry serves (e.g.
        // `@acme/private`), not the local alias. Callers that omit
        // an explicit alias (`pnpm add gh:@acme/foo`) still get the
        // right entry in `node_modules` and the lockfile.
        let result = build_resolve_result(BuildResolveResult {
            meta: &picked.meta,
            picked: &picked.version,
            spec: &spec,
            alias: Some(spec.name.as_str()),
            resolved_via: NAMED_REGISTRY_RESOLVED_VIA,
            registry,
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude.as_ref(),
            picked_manifest_cache: &self.picked_manifest_cache,
        })?;

        Ok(Some(result))
    }

    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        let mut wanted = query.wanted_dependency.clone();
        if wanted.bare_specifier.is_none() {
            wanted.bare_specifier = Some("latest".to_string());
        }
        let mut resolve_opts = opts.clone();
        if !query.compatible {
            resolve_opts.update = UpdateBehavior::Latest;
        }
        let result = self.resolve_impl(&wanted, &resolve_opts).await?;
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

    async fn pick_from_registry(
        &self,
        registry: &str,
        spec: &RegistryPackageSpec,
        opts: &ResolveOptions,
        optional: bool,
    ) -> Result<Option<PickedFromRegistry>, ResolveError> {
        let pick_opts = PickPackageOptions {
            registry,
            preferred_version_selectors: opts.preferred_versions.get(&spec.name),
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude.as_ref(),
            pick_lowest_version: opts.pick_lowest_version,
            include_latest_tag: opts.update == UpdateBehavior::Latest,
            dry_run: opts.dry_run,
            optional,
            update_checksums: opts.update_checksums,
        };

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
            retry_opts: self.retry_opts,
        };

        let pick_result = pick_package(&ctx, spec, &pick_opts)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;

        let Some(version) = pick_result.picked_package else {
            return Ok(None);
        };

        Ok(Some(PickedFromRegistry { meta: pick_result.meta, version }))
    }
}

#[cfg(test)]
mod tests;

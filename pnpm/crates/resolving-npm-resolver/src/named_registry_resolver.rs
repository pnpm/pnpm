//! Named-registry resolver.
//!
//! Parses a `<alias>:` specifier through
//! [`parse_named_registry_specifier_to_registry_package_spec`], looks
//! the alias up in the merged named-registries map, and picks the
//! version against that registry's URL. The result carries
//! `resolved_via = "named-registry"` and the scoped package name as
//! the alias, so an edge that declares no name of its own is installed
//! under its original name. An edge declared under a manifest key keeps
//! that key.
//!
//! Authentication piggybacks on the existing per-URL `.npmrc`
//! mechanism: a `//npm.pkg.github.com/:_authToken=...` entry takes
//! effect for `gh:` specifiers (and analogously for any user-configured
//! alias) because the resolver looks the auth header up by the
//! resolved registry URL, not the alias name.

use std::collections::{HashMap, HashSet};

use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, UpdateBehavior, WantedDependency,
};

use crate::{
    npm_resolver::{
        PickFromRegistryOptions, RegistryPick, no_matching_version, pick_from_registry_with_guard,
        swallowed_as_no_latest, validate_revision_selector,
    },
    parse_bare_specifier::{
        NamedRegistryPackageSpec, parse_named_registry_specifier_to_registry_package_spec,
    },
    pick_package::PackageMetaCache,
    pick_package_from_meta::RegistryPackageSpec,
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

/// Provenance tag emitted on [`ResolveResult::resolved_via`] when the
/// picker drove a `<alias>:` specifier through a configured named
/// registry.
const NAMED_REGISTRY_RESOLVED_VIA: &str = "named-registry";

/// Named-registry resolver.
///
/// One instance per install. Same plumbing as [`crate::NpmResolver`],
/// with the already-merged-and-validated map of named registries and a
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
    pub registries_by_prefix: HashMap<String, String>,
    /// Precomputed key set of [`Self::registries_by_prefix`]. The parser
    /// checks aliases against this set per call, so caching it
    /// avoids rebuilding the set for every resolve.
    pub registry_names: HashSet<String>,
    pub metadata: crate::RegistryMetadataClient<Cache>,
    pub format: crate::RegistryMetadataFormat,
    pub cache_policy: crate::MetadataCachePolicy,
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
        let Some(NamedRegistryPackageSpec { spec, registry_name }) =
            self.parse_specifier(wanted_dependency, opts, bare_specifier)?
        else {
            return Ok(None);
        };
        validate_revision_selector(&spec)?;

        // Defensive: should never trigger because the parser checks
        // the alias set first, but kept as a belt-and-braces guard.
        let Some(registry) = self.registries_by_prefix.get(&registry_name) else {
            return Ok(None);
        };

        let optional = wanted_dependency.optional.unwrap_or(false);
        let picked = match self.pick_from_registry(registry, &spec, opts, optional).await? {
            RegistryPick::Picked(picked) => picked,
            RegistryPick::NoMatchingVersion(meta) => {
                return Err(no_matching_version(wanted_dependency, registry, &meta));
            }
        };

        crate::npm_resolver::RegistryResolutionSource {
            resolved_via: NAMED_REGISTRY_RESOLVED_VIA,
            registry,
            registry_name: Some(registry_name.as_str()),
        }
        .build_result(
            &picked,
            &opts.policy,
            &self.metadata.picked_manifest_cache,
            crate::npm_resolver::ResolvedSpecifier::prefixed(
                wanted_dependency,
                opts,
                &spec,
                &format!("{registry_name}:"),
                &spec.name,
                &picked.version,
            ),
        )
        .map(Some)
    }

    fn parse_specifier(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        bare_specifier: &str,
    ) -> Result<Option<NamedRegistryPackageSpec>, ResolveError> {
        let default_tag = opts.version.default_tag.as_deref().unwrap_or("latest");

        let parsed = parse_named_registry_specifier_to_registry_package_spec(
            bare_specifier,
            &self.registry_names,
            wanted_dependency.alias.as_deref(),
            default_tag,
        )
        .map_err(|err| Box::new(err) as ResolveError)?;
        Ok(parsed)
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

    async fn pick_from_registry(
        &self,
        registry: &str,
        spec: &RegistryPackageSpec,
        opts: &ResolveOptions,
        optional: bool,
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
                checks: crate::npm_resolver::CandidateChecks::new(&opts.policy, None),
                policy: crate::PackagePickPolicy {
                    published_by: opts.policy.published_by,
                    published_by_exclude: opts.policy.published_by_exclude.as_ref(),
                    trust_policy: opts.policy.trust_policy,
                },
                request: crate::MetadataPickRequest {
                    dry_run: opts.refresh.dry_run,
                    optional,
                    refresh_metadata: opts.refresh.update != UpdateBehavior::Off,
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
}

#[cfg(test)]
mod tests;

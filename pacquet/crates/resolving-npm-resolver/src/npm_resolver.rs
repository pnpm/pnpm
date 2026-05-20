//! Pacquet port of pnpm's npm-registry resolver. Wraps
//! [`parse_bare_specifier`](crate::parse_bare_specifier) plus
//! [`pick_package`](crate::pick_package) behind the chain-friendly
//! [`Resolver`] trait so the default-resolver dispatcher can dispatch
//! npm-shaped dependencies through it.
//!
//! Mirrors upstream's
//! [`createNpmResolver` → `resolveNpm`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L192-L611)
//! pair: the struct owns the registry config + network handles + meta
//! cache; the trait implementation parses the bare specifier, picks a
//! version, and maps the result to [`ResolveResult`].
//!
//! Out of scope for this port:
//!
//! - **Workspace resolution.** `workspace:` specs return `Ok(None)` so
//!   the dispatcher falls through to the workspace resolver when that
//!   crate lands.
//! - **`peekManifestFromStore` fast path.** Upstream short-circuits a
//!   registry fetch when the lockfile-pinned tarball is already in the
//!   store. Pacquet today goes through the picker unconditionally;
//!   restoring the fast path is a separate item.
//! - **Trust-policy enforcement.** The resolver-side
//!   `failIfTrustDowngraded` call is wired through the verifier crate
//!   only; the resolver path doesn't enforce it yet.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use pacquet_config::version_policy::PackageVersionPolicy;
use pacquet_lockfile::{
    LockfileResolution, PkgName, PkgNameVer, RegistryResolution, TarballResolution,
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolutionPolicyViolation, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, UpdateBehavior, WantedDependency,
};

use crate::{
    named_registry::pick_registry_for_package,
    parse_bare_specifier::parse_bare_specifier,
    pick_package::{PackageMetaCache, PickPackageContext, PickPackageOptions, pick_package},
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

/// npm-registry resolver.
///
/// One instance per install. Mirrors upstream's
/// [`createNpmResolver`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L192-L289)
/// factory return value: registries map, named-registry overrides,
/// throttled HTTP client, auth-header table, on-disk metadata mirror
/// root, and the install-shared metadata cache the picker reads
/// through.
pub struct NpmResolver<Cache: PackageMetaCache> {
    /// `default` plus per-scope (`@scope`) entries. The keys mirror
    /// pnpm's `Registries` shape; the picker consults the `default`
    /// entry as the install-wide default and the scope entry when the
    /// resolved package name carries one. Pacquet today only populates
    /// `default` — per-scope wiring lands when `.npmrc`'s
    /// `<scope>:registry` parsing does.
    pub registries: HashMap<String, String>,
    /// User-supplied named-registry aliases (e.g. `gh:` →
    /// `https://npm.pkg.github.com/`). Merged with
    /// [`crate::BUILTIN_NAMED_REGISTRIES`] at construction. Today
    /// only consulted by the named-registry resolver (out of scope
    /// for this port); kept here so the install layer can build one
    /// resolver instance with the full registry view.
    pub named_registries: HashMap<String, String>,
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    pub meta_cache: Arc<Cache>,
    /// Root of the on-disk metadata mirror. `None` disables every
    /// disk read/write — the picker goes straight to the network on
    /// each cache miss.
    pub cache_dir: Option<PathBuf>,
    pub offline: bool,
    pub prefer_offline: bool,
    pub ignore_missing_time_field: bool,
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
        let registry = self.pick_registry(wanted_dependency.alias.as_deref());

        // `workspace:` is owned by the workspace resolver. Decline so
        // the chain dispatches there once that crate lands.
        if wanted_dependency
            .bare_specifier
            .as_deref()
            .is_some_and(|bare| bare.starts_with("workspace:"))
        {
            return Ok(None);
        }

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

        let pick_opts = PickPackageOptions {
            registry: &registry,
            preferred_version_selectors: opts.preferred_versions.get(&spec.name),
            published_by: opts.published_by,
            published_by_exclude: opts.published_by_exclude.as_ref(),
            pick_lowest_version: opts.pick_lowest_version,
            include_latest_tag: opts.update == UpdateBehavior::Latest,
            dry_run: opts.dry_run,
        };

        let ctx = PickPackageContext {
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            meta_cache: self.meta_cache.as_ref(),
            cache_dir: self.cache_dir.as_deref(),
            offline: self.offline,
            prefer_offline: self.prefer_offline,
            ignore_missing_time_field: self.ignore_missing_time_field,
        };

        let pick_result = pick_package(&ctx, &spec, &pick_opts)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;

        let Some(picked) = pick_result.picked_package else {
            return Ok(None);
        };

        let result = build_resolve_result(
            &pick_result.meta,
            &picked,
            &spec,
            wanted_dependency.alias.as_deref(),
            opts.published_by,
            opts.published_by_exclude.as_ref(),
        )?;

        Ok(Some(result))
    }

    /// Latest-version companion. Mirrors upstream's
    /// [`createResolveLatest`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L323-L353)
    /// closure: feed `wanted.bareSpecifier ?? 'latest'` plus
    /// `update: 'latest'` (or the original opts under `compatible`) back
    /// through `resolve`, then return the picked manifest.
    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        let bare_specifier =
            query.wanted_dependency.bare_specifier.clone().unwrap_or_else(|| "latest".to_string());
        let wanted = WantedDependency {
            alias: query.wanted_dependency.alias.clone(),
            bare_specifier: Some(bare_specifier),
            ..WantedDependency::default()
        };
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

    fn pick_registry(&self, alias: Option<&str>) -> String {
        match alias {
            Some(name) => pick_registry_for_package(&self.registries, name),
            None => self.registries.get("default").cloned().unwrap_or_default(),
        }
    }
}

/// `bare_specifier` is absent but `alias` is present: synthesize a tag
/// spec pointing at the default tag, mirroring upstream's
/// [`defaultTagForAlias`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L1000-L1006).
fn default_tag_spec(alias: &str, default_tag: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: alias.to_string(),
        fetch_spec: default_tag.to_string(),
        spec_type: RegistryPackageSpecType::Tag,
        normalized_bare_specifier: None,
    }
}

fn build_resolve_result(
    meta: &Package,
    picked: &PackageVersion,
    spec: &RegistryPackageSpec,
    alias: Option<&str>,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Result<ResolveResult, ResolveError> {
    let pkg_name =
        PkgName::parse(picked.name.as_str()).map_err(|err| Box::new(err) as ResolveError)?;
    let id = PkgNameVer::new(pkg_name.clone(), picked.version.clone());
    let integrity = picked.dist.integrity.clone();
    let tarball = picked.dist.tarball.clone();
    let resolution = if let Some(integrity) = integrity.clone() {
        if tarball.is_empty() {
            // Registry resolutions carry only integrity; the tarball URL
            // is reconstructed at fetch time.
            LockfileResolution::Registry(RegistryResolution { integrity })
        } else {
            LockfileResolution::Tarball(TarballResolution {
                tarball: tarball.clone(),
                integrity: Some(integrity),
                git_hosted: None,
                path: None,
            })
        }
    } else {
        LockfileResolution::Tarball(TarballResolution {
            tarball: tarball.clone(),
            integrity: None,
            git_hosted: None,
            path: None,
        })
    };
    let published_at = meta.published_at(&picked.version.to_string()).map(str::to_string);
    let manifest = serde_json::to_value(picked).ok();
    let policy_violation = detect_min_release_age_violation(
        &pkg_name,
        &picked.version.to_string(),
        published_at.as_deref(),
        &resolution,
        published_by,
        published_by_exclude,
    );
    Ok(ResolveResult {
        id,
        latest: meta.dist_tag("latest").map(str::to_string),
        published_at,
        manifest,
        resolution,
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: spec.normalized_bare_specifier.clone(),
        alias: alias.map(str::to_string),
        policy_violation,
    })
}

/// Resolver-time `minimumReleaseAge` check. Returns a violation entry
/// when the picked version's publish timestamp falls past the policy
/// cutoff and isn't excluded by name/version. Mirrors upstream's
/// [`detectMinReleaseAgeViolation`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L1023-L1044).
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
        use pacquet_config::version_policy::PolicyMatch;
        match policy.matches(&name.to_string()) {
            PolicyMatch::AnyVersion => return None,
            PolicyMatch::ExactVersions(versions) if versions.iter().any(|v| v == version) => {
                return None;
            }
            _ => {}
        }
    }
    let parsed = DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Utc);
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

#[cfg(test)]
mod tests;

//! Pacquet port of pnpm's npm-registry resolver. Wraps
//! [`parse_bare_specifier`](crate::parse_bare_specifier()) plus
//! [`pick_package`](crate::pick_package()) behind the chain-friendly
//! [`Resolver`] trait so the default-resolver dispatcher can dispatch
//! npm-shaped dependencies through it.
//!
//! Mirrors upstream's
//! [`createNpmResolver` → `resolveNpm`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/index.ts#L192-L611)
//! pair: the struct owns the registry config + network handles + meta
//! cache; the trait implementation parses the bare specifier, picks a
//! version, and maps the result to [`ResolveResult`].
//!
//! Workspace handling intentionally lives on the npm-resolver side
//! (mirroring upstream): non-path `workspace:` specs route through
//! [`try_resolve_from_workspace`](crate::try_resolve_from_workspace())
//! to a `link:` / `file:` resolution against the install's workspace
//! package map; the path-relative forms (`workspace:./foo`,
//! `workspace:../bar`) return `Ok(None)` so the local-resolver in the
//! chain claims them.
//!
//! Out of scope for this port:
//!
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
use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolutionPolicyViolation, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, UpdateBehavior, WantedDependency,
};

use crate::{
    named_registry::pick_registry_for_package,
    parse_bare_specifier::{parse_bare_specifier, parse_jsr_specifier_to_registry_package_spec},
    pick_package::{PackageMetaCache, PickPackageContext, PickPackageOptions, pick_package},
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
    resolve_from_workspace::{ResolveFromWorkspaceOptions, try_resolve_from_workspace},
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

/// Default `@jsr` registry URL. Mirrors upstream's
/// [`DEFAULT_REGISTRIES['@jsr']`](https://github.com/pnpm/pnpm/blob/1627943d2a/config/normalize-registries/src/index.ts#L5-L8):
/// every `normalizeRegistries` call always populates `'@jsr'`, so the
/// TS dispatcher reads `ctx.registries['@jsr']!` unconditionally. This
/// constant is the fallback for pacquet callers that haven't routed
/// the `@jsr` entry through their `registries` map yet.
const DEFAULT_JSR_REGISTRY: &str = "https://npm.jsr.io/";

/// Provenance tag for [`ResolveResult::resolved_via`] when the picker
/// drove a JSR-prefixed specifier through the `@jsr` registry. Mirrors
/// upstream's
/// [`resolveJsr`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/index.ts#L629).
const JSR_REGISTRY_RESOLVED_VIA: &str = "jsr-registry";

/// Provenance tag for npm-registry resolutions. Mirrors upstream's
/// [`resolveNpm`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/index.ts#L595-L601).
const NPM_REGISTRY_RESOLVED_VIA: &str = "npm-registry";

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
    /// Install-wide bias toward full metadata. Threaded through to
    /// [`PickPackageContext::full_metadata`]. Mirrors upstream's
    /// [`ctx.fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L175).
    pub full_metadata: bool,
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

        // `workspace:` is intercepted before the npm pick — only the
        // path-relative forms (`workspace:./foo`, `workspace:../bar`)
        // fall through here so the local-resolver in the chain claims
        // them. Everything else routes through
        // [`try_resolve_from_workspace`], mirroring upstream's
        // [`resolveNpm`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L412-L429)
        // gate.
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
                workspace_packages: opts.workspace_packages.as_ref(),
                inject_workspace_packages: opts.inject_workspace_packages,
            };
            return try_resolve_from_workspace(wanted_dependency, &ws_opts)
                .map_err(|err| Box::new(err) as ResolveError);
        }

        // `jsr:` resolves through the `@jsr` registry under the
        // `@jsr/<scope>__<name>` folded name. Mirrors upstream's
        // [`resolveJsr`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/index.ts#L613-L632),
        // which is dispatched alongside `resolveNpm` from the same
        // factory.
        if let Some(bare) = wanted_dependency.bare_specifier.as_deref()
            && bare.starts_with("jsr:")
        {
            return self.resolve_jsr_impl(wanted_dependency, opts, bare, default_tag).await;
        }

        // Pick registry from `(alias, bare_specifier)` so an npm-alias
        // entry like `"foo": "npm:@scope/bar@^1"` routes through
        // `registries[@scope]` instead of the alias's own scope.
        // Mirrors upstream's
        // [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/pick-registry-for-package/src/index.ts).
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

        let optional = wanted_dependency.optional.unwrap_or(false);
        let picked = match self.pick_from_registry(&registry, &spec, opts, optional).await? {
            Some(picked) => picked,
            None => return Ok(None),
        };

        let result = build_resolve_result(
            &picked.meta,
            &picked.version,
            &spec,
            wanted_dependency.alias.as_deref(),
            NPM_REGISTRY_RESOLVED_VIA,
            opts.published_by,
            opts.published_by_exclude.as_ref(),
        )?;

        Ok(Some(result))
    }

    /// JSR counterpart to the npm path. Mirrors upstream's
    /// [`resolveJsr`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/index.ts#L613-L632):
    /// runs the JSR-specifier parser, picks against the `@jsr`
    /// registry, then stamps `resolved_via = "jsr-registry"` and
    /// `alias = spec.jsr_pkg_name` on the result so the install layer
    /// records the dependency under its JSR-style name.
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

        let registry =
            self.registries.get("@jsr").map(String::as_str).unwrap_or(DEFAULT_JSR_REGISTRY);

        let optional = wanted_dependency.optional.unwrap_or(false);
        let picked = match self.pick_from_registry(registry, &jsr_spec.spec, opts, optional).await?
        {
            Some(picked) => picked,
            None => return Ok(None),
        };

        let result = build_resolve_result(
            &picked.meta,
            &picked.version,
            &jsr_spec.spec,
            Some(jsr_spec.jsr_pkg_name.as_str()),
            JSR_REGISTRY_RESOLVED_VIA,
            opts.published_by,
            opts.published_by_exclude.as_ref(),
        )?;

        Ok(Some(result))
    }

    /// Common picker invocation shared by [`Self::resolve_impl`] and
    /// [`Self::resolve_jsr_impl`]. Returns `Ok(None)` when the picker
    /// finds no matching version so each caller can fold that into
    /// its own `Ok(None)` short-circuit.
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
        };

        let ctx = PickPackageContext {
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            meta_cache: self.meta_cache.as_ref(),
            cache_dir: self.cache_dir.as_deref(),
            offline: self.offline,
            prefer_offline: self.prefer_offline,
            ignore_missing_time_field: self.ignore_missing_time_field,
            full_metadata: self.full_metadata,
        };

        let pick_result = pick_package(&ctx, spec, &pick_opts)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;

        let Some(version) = pick_result.picked_package else {
            return Ok(None);
        };

        Ok(Some(PickedFromRegistry { meta: pick_result.meta, version }))
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
        // Mirror upstream's `createResolveLatest`: only the
        // `bare_specifier` is rewritten (synthesized to the default
        // tag when missing). Cloning the rest of the wanted
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

/// Picker output threaded through to [`build_resolve_result`].
pub(crate) struct PickedFromRegistry {
    pub(crate) meta: Package,
    pub(crate) version: PackageVersion,
}

pub(crate) fn build_resolve_result(
    meta: &Package,
    picked: &PackageVersion,
    spec: &RegistryPackageSpec,
    alias: Option<&str>,
    resolved_via: &str,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Result<ResolveResult, ResolveError> {
    let pkg_name =
        PkgName::parse(picked.name.as_str()).map_err(|err| Box::new(err) as ResolveError)?;
    let name_ver = PkgNameVer::new(pkg_name.clone(), picked.version.clone());
    let id = (&name_ver).into();
    // The picker always carries a tarball URL on its `dist` payload —
    // every npm registry serves `dist.tarball` on a successful pick
    // and pacquet's deserializer requires it (`dist.tarball: String`,
    // not `Option`). Always emit `Tarball`, never `Registry`. The
    // install side's `extract_tarball` only handles `Tarball`, so
    // mixing the two shapes would force a Registry → URL
    // reconstruction with no payoff: at resolve time we already have
    // the URL the install path needs.
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: picked.dist.tarball.clone(),
        integrity: picked.dist.integrity.clone(),
        git_hosted: None,
        path: None,
    });
    let published_at = meta.published_at(&picked.version.to_string()).map(str::to_string);
    let manifest = Some(serde_json::to_value(picked).map_err(|err| Box::new(err) as ResolveError)?);
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
        name_ver: Some(name_ver),
        latest: meta.dist_tag("latest").map(str::to_string),
        published_at,
        manifest,
        resolution,
        resolved_via: resolved_via.to_string(),
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
            PolicyMatch::ExactVersions(versions)
                if versions.iter().any(|exact| exact == version) =>
            {
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

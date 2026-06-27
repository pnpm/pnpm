//! Pacquet port of upstream's
//! [`resolveBunRuntime` / `resolveLatestBunRuntime`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/bun-resolver/src/index.ts#L24-L92).

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{LockfileResolution, VariationsResolution};
use pacquet_network::ThrottledClient;
use pacquet_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, UpdateBehavior, WantedDependency,
};

use crate::read_bun_assets::{ReadBunAssetsError, read_bun_assets};

const RESOLVED_VIA: &str = "github.com/oven-sh/bun";
const BARE_SPEC_PREFIX: &str = "runtime:";

/// Errors emitted by [`BunResolver`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BunResolverError {
    #[display("Could not resolve Bun version specified as {spec}")]
    #[diagnostic(code(BUN_RESOLUTION_FAILURE))]
    ResolutionFailure {
        #[error(not(source))]
        spec: String,
    },

    #[diagnostic(transparent)]
    ReadAssets(#[error(source)] ReadBunAssetsError),
}

/// Bun runtime resolver entry point.
///
/// Owns the throttled HTTP client (for the GitHub-release SHASUMS
/// fetch) and an `Arc<dyn Resolver>` for the npm resolver that
/// version selection delegates to.
pub struct BunResolver {
    pub http_client: Arc<ThrottledClient>,
    pub npm_resolver: Arc<dyn Resolver>,
}

impl BunResolver {
    pub fn new(http_client: Arc<ThrottledClient>, npm_resolver: Arc<dyn Resolver>) -> Self {
        Self { http_client, npm_resolver }
    }
}

impl Resolver for BunResolver {
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

impl BunResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        _opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(version_spec) = bare_runtime_spec(wanted_dependency, "bun") else {
            return Ok(None);
        };
        let version_spec = normalize_runtime_spec(version_spec);

        let npm_result = self
            .npm_resolver
            .resolve(
                &WantedDependency {
                    alias: wanted_dependency.alias.clone(),
                    bare_specifier: Some(version_spec.to_string()),
                    ..wanted_dependency.clone()
                },
                &ResolveOptions::default(),
            )
            .await?;
        let version = npm_result
            .as_ref()
            .and_then(|result| result.name_ver.as_ref().map(|name_ver| name_ver.suffix.to_string()))
            .ok_or_else(|| {
                Box::new(BunResolverError::ResolutionFailure { spec: version_spec.to_string() })
                    as ResolveError
            })?;

        let variants = read_bun_assets(&self.http_client, &version)
            .await
            .map_err(|err| Box::new(BunResolverError::ReadAssets(err)) as ResolveError)?;
        let resolution = LockfileResolution::Variations(VariationsResolution { variants });
        let manifest = serde_json::json!({
            "name": "bun",
            "version": version,
            "bin": bun_bin_for_current_os(current_platform()),
        });
        Ok(Some(ResolveResult {
            id: format!("bun@runtime:{version}").into(),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(manifest)),
            resolution,
            resolved_via: RESOLVED_VIA.to_string(),
            normalized_bare_specifier: Some(format!("runtime:{version_spec}")),
            alias: wanted_dependency.alias.clone(),
            policy_violation: None,
        }))
    }

    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        let Some(manifest_spec) = bare_runtime_spec(&query.wanted_dependency, "bun") else {
            return Ok(None);
        };
        let version_spec =
            if query.compatible { normalize_runtime_spec(manifest_spec) } else { "latest" }
                .to_string();
        let mut resolve_opts = opts.clone();
        if !query.compatible {
            resolve_opts.update = UpdateBehavior::Latest;
        }
        let npm_result = self
            .npm_resolver
            .resolve(
                &WantedDependency {
                    alias: Some("bun".to_string()),
                    bare_specifier: Some(version_spec),
                    ..WantedDependency::default()
                },
                &resolve_opts,
            )
            .await?;
        let Some(npm_result) = npm_result else {
            return Ok(Some(LatestInfo::default()));
        };
        if npm_result
            .policy_violation
            .as_ref()
            .is_some_and(|violation| violation.code == MINIMUM_RELEASE_AGE_VIOLATION_CODE)
        {
            return Ok(Some(LatestInfo::default()));
        }
        let Some(name_ver) = npm_result.name_ver else {
            return Ok(Some(LatestInfo::default()));
        };
        Ok(Some(LatestInfo {
            latest_manifest: Some(std::sync::Arc::new(serde_json::json!({
                "name": "bun",
                "version": name_ver.suffix.to_string(),
            }))),
        }))
    }
}

fn bare_runtime_spec<'a>(wanted: &'a WantedDependency, expected_alias: &str) -> Option<&'a str> {
    if wanted.alias.as_deref() != Some(expected_alias) {
        return None;
    }
    wanted.bare_specifier.as_deref().and_then(|spec| spec.strip_prefix(BARE_SPEC_PREFIX))
}

fn normalize_runtime_spec(version_spec: &str) -> &str {
    let version_spec = version_spec.trim();
    if version_spec.is_empty() { "latest" } else { version_spec }
}

fn bun_bin_for_current_os(platform: &str) -> &'static str {
    if platform == "win32" { "bun.exe" } else { "bun" }
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        other => other,
    }
}

#[cfg(test)]
mod tests;

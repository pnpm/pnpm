//! Resolves `yarn@runtime:<spec>` dependencies against `yarnpkg/zpm`.

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_lockfile::{LockfileResolution, VariationsResolution};
use pnpm_network::{ThrottledClient, redact_and_sanitize};
use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

use crate::read_yarn_releases::{
    ReadYarnReleasesError, YarnRelease, asset_variants, fetch_yarn_releases,
};

const RESOLVED_VIA: &str = "github.com/yarnpkg/zpm";
const BARE_SPEC_PREFIX: &str = "runtime:";

#[derive(Debug, Display, Error, Diagnostic)]
pub enum YarnResolverError {
    #[display("Could not resolve Yarn version specified as {spec}")]
    #[diagnostic(code(ERR_PNPM_YARN_RESOLUTION_FAILURE))]
    ResolutionFailure {
        #[error(not(source))]
        spec: String,
    },

    #[diagnostic(transparent)]
    ReadReleases(#[error(source)] ReadYarnReleasesError),
}

/// Yarn 6 resolver entry point. Owns the throttled HTTP client the
/// release list is fetched with; unlike the npm-published Yarn lines,
/// nothing here goes through a registry.
pub struct YarnResolver {
    pub http_client: Arc<ThrottledClient>,
    /// The release list, fetched at most once per resolver. One command
    /// can resolve Yarn more than once — a resolve and a latest probe, or
    /// several importers pinning it — and the list is one unconditional
    /// GitHub API request, which is rate-limited for an unauthenticated
    /// caller.
    releases: tokio::sync::OnceCell<Arc<Vec<YarnRelease>>>,
}

impl YarnResolver {
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, releases: tokio::sync::OnceCell::new() }
    }

    async fn releases(&self) -> Result<&[YarnRelease], ReadYarnReleasesError> {
        self.releases
            .get_or_try_init(|| async {
                fetch_yarn_releases(&self.http_client).await.map(Arc::new)
            })
            .await
            .map(|releases| releases.as_slice())
    }
}

impl Resolver for YarnResolver {
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
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(self.resolve_latest_impl(query))
    }
}

impl YarnResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        _opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(version_spec) = bare_runtime_spec(wanted_dependency) else {
            return Ok(None);
        };
        let releases = self
            .releases()
            .await
            .map_err(|error| Box::new(YarnResolverError::ReadReleases(error)) as ResolveError)?;
        let release = pick_release(releases, version_spec).ok_or_else(|| {
            // The specifier comes from a manifest, so it can carry
            // credentials — the message a user sees must not.
            let spec = redact_and_sanitize(version_spec);
            Box::new(YarnResolverError::ResolutionFailure { spec }) as ResolveError
        })?;
        let variants = asset_variants(release)
            .map_err(|error| Box::new(YarnResolverError::ReadReleases(error)) as ResolveError)?;

        let version = release.version.clone();
        let manifest = serde_json::json!({
            "name": "yarn",
            "version": version,
            "bin": yarn_bin_for_current_os(),
        });
        Ok(Some(ResolveResult {
            id: format!("yarn@runtime:{version}").into(),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(manifest)),
            resolution: LockfileResolution::Variations(VariationsResolution { variants }),
            resolved_via: RESOLVED_VIA.to_string(),
            normalized_bare_specifier: Some(format!("runtime:{version_spec}")),
            alias: wanted_dependency.alias.clone(),
            policy_violation: None,
        }))
    }

    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        if bare_runtime_spec(&query.wanted_dependency).is_none() {
            return Ok(None);
        }
        let releases = self
            .releases()
            .await
            .map_err(|error| Box::new(YarnResolverError::ReadReleases(error)) as ResolveError)?;
        let Some(release) = pick_release(releases, "latest") else {
            return Ok(Some(LatestInfo::default()));
        };
        Ok(Some(LatestInfo {
            latest_manifest: Some(Arc::new(serde_json::json!({
                "name": "yarn",
                "version": release.version,
            }))),
        }))
    }
}

/// The exact version `version_spec` selects, without installing it.
///
/// Recording which Yarn a project uses needs the version the release list
/// resolves to, because the `packageManager` field holds an exact version
/// rather than a range.
pub async fn resolve_yarn_version(
    http_client: &ThrottledClient,
    version_spec: &str,
) -> Result<String, YarnResolverError> {
    let releases =
        fetch_yarn_releases(http_client).await.map_err(YarnResolverError::ReadReleases)?;
    pick_release(&releases, version_spec).map(|release| release.version.clone()).ok_or_else(|| {
        YarnResolverError::ResolutionFailure { spec: redact_and_sanitize(version_spec) }
    })
}

/// The newest release satisfying `version_spec`.
///
/// Yarn 6 is still a release candidate, so a plain range like `^6.0.0`
/// matches nothing under semver's prerelease rule. Rather than refuse to
/// install the only Yarn 6 that exists, an unsatisfied range is retried
/// against each candidate's release version with its prerelease tag
/// dropped — the same allowance the `packageManager` version check makes.
pub(crate) fn pick_release<'a>(
    releases: &'a [YarnRelease],
    version_spec: &str,
) -> Option<&'a YarnRelease> {
    let version_spec = version_spec.trim();
    let mut candidates: Vec<(node_semver::Version, &YarnRelease)> = releases
        .iter()
        .filter_map(|release| Some((node_semver::Version::parse(&release.version).ok()?, release)))
        .collect();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    if version_spec.is_empty() || version_spec == "latest" || version_spec == "*" {
        return candidates.first().map(|(_, release)| *release);
    }
    let range = node_semver::Range::parse(version_spec).ok()?;
    candidates
        .iter()
        .find(|(version, _)| version.satisfies(&range))
        .or_else(|| {
            candidates.iter().find(|(version, _)| without_prerelease(version).satisfies(&range))
        })
        .map(|(_, release)| *release)
}

fn without_prerelease(version: &node_semver::Version) -> node_semver::Version {
    node_semver::Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: version.build.clone(),
    }
}

fn bare_runtime_spec(wanted: &WantedDependency) -> Option<&str> {
    if wanted.alias.as_deref() != Some("yarn") {
        return None;
    }
    wanted.bare_specifier.as_deref().and_then(|spec| spec.strip_prefix(BARE_SPEC_PREFIX))
}

/// The archive member the manifest advertises as the engine's bin. See
/// `read_yarn_releases::yarn_bin_path` for why it is not the `yarn`
/// launcher sitting beside it.
fn yarn_bin_for_current_os() -> &'static str {
    if std::env::consts::OS == "windows" { "yarn-bin.exe" } else { "yarn-bin" }
}

#[cfg(test)]
mod tests;

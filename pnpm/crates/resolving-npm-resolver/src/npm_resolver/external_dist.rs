//! Split packuments (proof of concept). A registry that serves the
//! `application/vnd.pnpr.install-v2+json` form may leave a version's dist
//! facts (integrities, sizes, bins, variant archives) out of a large
//! packument and mark the version `dist.pnpr.details: "external"`. They are
//! then in the immutable version document `<registry><name>/<version>`,
//! fetched here once the version is picked.

use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use pnpm_network::send_with_retry;
use pnpm_registry::{PackageDistribution, PackageVersion};
use pnpm_resolving_resolver_base::ResolveError;
use reqwest::header;
use tokio::sync::OnceCell;

use crate::{
    MetadataHttpClient, PackageMetaCache, PickPackageError, pick_package::PickPackageContext,
};

/// Version documents never change, so one fetch per URL serves the install.
static VERSION_DIST: LazyLock<DashMap<String, Arc<OnceCell<Arc<PackageDistribution>>>>> =
    LazyLock::new(DashMap::new);

/// Fill in the dist facts of a picked version that its packument left in the
/// version document.
pub(crate) async fn with_external_dist<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: Arc<PackageVersion>,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    if !version.dist.is_external() {
        return Ok(version);
    }
    let url = format!("{}/{}/{}", registry.trim_end_matches('/'), version.name, version.version);
    let cell = Arc::clone(
        VERSION_DIST
            .entry(url.clone())
            .or_default()
            .value(),
    );
    let dist = cell
        .get_or_try_init(|| fetch_version_dist(ctx.metadata.http, &url, &version.name))
        .await
        .map_err(|error| PickPackageError::VersionDocument { url, error: error.to_string() })?;
    let mut version = PackageVersion::clone(&version);
    version.dist = PackageDistribution::clone(dist);
    Ok(Arc::new(version))
}

async fn fetch_version_dist(
    http: MetadataHttpClient<'_>,
    url: &str,
    name: &str,
) -> Result<Arc<PackageDistribution>, ResolveError> {
    let (_guard, response) = send_with_retry(http.http_client, url, http.retry_opts, |client| {
        let request = client.get(url).header(header::ACCEPT, "application/json");
        match http.auth_headers.for_url_with_package(url, Some(name)) {
            Some(value) => request.header(header::AUTHORIZATION, value),
            None => request,
        }
    })
    .await?;
    let bytes = response.error_for_status()?.bytes().await?;
    let version: PackageVersion = serde_json::from_slice(&bytes)?;
    Ok(Arc::new(version.dist))
}

#[cfg(test)]
mod tests;

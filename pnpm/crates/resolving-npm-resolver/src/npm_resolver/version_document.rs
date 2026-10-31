//! Split packuments (proof of concept). A registry that serves the
//! `application/vnd.pnpr.install-v2+json` form may leave a version out of a
//! large packument, keeping only what picking reads (its `deprecated` notice,
//! trust flags, and provenance), and mark it `dist.pnpr.details: "external"`.
//! The whole version is then in its immutable version document
//! `<registry><name>/<version>`, fetched here once the version is picked.

use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use pnpm_network::send_with_retry;
use pnpm_registry::PackageVersion;
use pnpm_resolving_resolver_base::ResolveError;
use reqwest::header;
use tokio::sync::OnceCell;

use crate::{
    MetadataHttpClient, PackageMetaCache, PickPackageError, pick_package::PickPackageContext,
};

/// Asks for the version as the split packument would have listed it.
const ACCEPT_VERSION_DOC: &str =
    "application/vnd.pnpr.install-v2+json; q=1.0, application/json; q=0.9";

/// Version documents never change, so one fetch per URL serves the install.
static VERSION_DOCS: LazyLock<DashMap<String, Arc<OnceCell<Arc<PackageVersion>>>>> =
    LazyLock::new(DashMap::new);

/// The picked version, read from its version document when its packument
/// left it there.
pub(crate) async fn with_version_document<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: Arc<PackageVersion>,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    if !version.dist.is_external() {
        return Ok(version);
    }
    let url = format!("{}/{}/{}", registry.trim_end_matches('/'), version.name, version.version);
    let cell = Arc::clone(
        VERSION_DOCS
            .entry(url.clone())
            .or_default()
            .value(),
    );
    cell.get_or_try_init(|| fetch_version_document(ctx.metadata.http, &url, &version))
        .await
        .cloned()
        .map_err(|error| PickPackageError::VersionDocument { url, error })
}

async fn fetch_version_document(
    http: MetadataHttpClient<'_>,
    url: &str,
    picked: &PackageVersion,
) -> Result<Arc<PackageVersion>, String> {
    let fetched = async {
        let (_guard, response) =
            send_with_retry(http.http_client, url, http.retry_opts, |client| {
                let request = client.get(url).header(header::ACCEPT, ACCEPT_VERSION_DOC);
                match http.auth_headers.for_url_with_package(url, Some(&picked.name)) {
                    Some(value) => request.header(header::AUTHORIZATION, value),
                    None => request,
                }
            })
            .await?;
        let bytes = response.error_for_status()?.bytes().await?;
        Ok::<PackageVersion, ResolveError>(serde_json::from_slice(&bytes)?)
    }
    .await
    .map_err(|error| error.to_string())?;
    if fetched.name != picked.name || fetched.version != picked.version {
        return Err(format!("it describes {}@{}", fetched.name, fetched.version));
    }
    if fetched.dist.is_external() {
        return Err("it has no details of the version either".to_string());
    }
    Ok(Arc::new(fetched))
}

#[cfg(test)]
mod tests;

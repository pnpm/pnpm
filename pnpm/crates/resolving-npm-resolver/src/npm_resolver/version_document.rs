//! Split packuments (proof of concept). A registry that serves the
//! `application/vnd.pnpr.install-v2+json` form may list a version only by
//! what picking reads and what may change after publication: its
//! `deprecated` notice, trust evidence, selected revision, and recent publish
//! time (see
//! `pnpm_registry::add_other_versions`). The rest of the version is in
//! its release record `<registry><name>/<version>+r<revision>`, which never
//! changes, fetched here once the version is picked.

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

/// Release records never change, so one fetch per URL serves the install.
static VERSION_DOCS: LazyLock<DashMap<String, Arc<OnceCell<Arc<PackageVersion>>>>> =
    LazyLock::new(DashMap::new);

/// The picked version, read from its release record when its packument left
/// it there. What may change after publication comes from the packument.
pub(crate) async fn with_version_document<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: Arc<PackageVersion>,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    if !version.dist.is_external() {
        return Ok(version);
    }
    let revision = version.dist.revision
        .as_ref()
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let url = format!(
        "{}/{}/{}+r{revision}",
        registry.trim_end_matches('/'),
        version.name,
        version.version,
    );
    let cell = Arc::clone(
        VERSION_DOCS
            .entry(url.clone())
            .or_default()
            .value(),
    );
    let record = cell
        .get_or_try_init(|| fetch_version_document(ctx.metadata.http, &url, &version))
        .await
        .map_err(|error| PickPackageError::VersionDocument { url, error })?;
    let mut picked = PackageVersion::clone(record);
    picked.deprecated.clone_from(&version.deprecated);
    picked.npm_user.clone_from(&version.npm_user);
    picked.dist.attestations.clone_from(&version.dist.attestations);
    picked.dist.revision.clone_from(&version.dist.revision);
    Ok(Arc::new(picked))
}

async fn fetch_version_document(
    http: MetadataHttpClient<'_>,
    url: &str,
    picked: &PackageVersion,
) -> Result<Arc<PackageVersion>, String> {
    let fetched = async {
        let (_guard, response) =
            send_with_retry(http.http_client, url, http.retry_opts, |client| {
                let request = client.get(url).header(header::ACCEPT, "application/json");
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

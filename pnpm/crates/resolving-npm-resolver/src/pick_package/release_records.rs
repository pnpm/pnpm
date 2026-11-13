//! Release records of versions picked from pnpr's own package document
//! (see `pnpm_registry::parse_install_v2`). An entry of that document may give
//! a version only by what picking reads and what may change after
//! publication; the rest of the version, and every revision other than the
//! selected one, is its release record,
//! `<registry><name>/<version>+r<revision>`. A record never changes, so it is
//! fetched once per install, shared between the picks that need it, and
//! cached on disk with no expiry.

use std::{path::PathBuf, sync::Arc};

use pnpm_network::{
    LocalResponse, read_limited_body, retry_async, send_with_retry, should_retry_status,
};
use pnpm_registry::{InstallV2Entry, PackageVersion};
use reqwest::header;
use tokio::sync::OnceCell;

use super::{PackageMetaCache, PickPackageContext, PickPackageError, serves_pnpr_format};
use crate::{
    RegistryRevisionSelector,
    mirror::{RELEASE_RECORDS_DIR, release_record_path, save_meta, scoped_meta_dir},
    registry_url::to_registry_url,
};

/// The largest release record read: one version's manifest and the facts
/// about its archives, far below this, so a larger body is refused rather than
/// buffered.
const RELEASE_RECORD_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// The in-flight and fetched records of one install, keyed by URL.
pub(super) type ReleaseRecords = dashmap::DashMap<String, Arc<OnceCell<Arc<PackageVersion>>>>;

/// One release record to read, from disk or from the registry.
pub(crate) struct ReleaseRecordRequest<'ctx> {
    pub cache_dir: Option<&'ctx std::path::Path>,
    pub forbids_registry: bool,
    pub http: crate::MetadataHttpClient<'ctx>,
    pub registry: &'ctx str,
    pub url: &'ctx str,
    pub name: &'ctx str,
    pub version: &'ctx str,
    pub revision: u64,
}

#[derive(Debug)]
pub(crate) enum ReleaseRecordError {
    NotFound(String),
    Other(String),
}

impl ReleaseRecordError {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::NotFound(message) | Self::Other(message) => message,
        }
    }
}

impl ReleaseRecordRequest<'_> {
    pub(crate) async fn run(&self) -> Result<Arc<PackageVersion>, ReleaseRecordError> {
        let path = self.disk_path();
        let cached = match path.as_deref() {
            Some(path) => tokio::fs::read(path).await.ok(),
            None => None,
        };
        if let Some(record) = cached.and_then(|body| self.parse(&body).ok()) {
            return Ok(record);
        }
        if self.forbids_registry {
            return Err(ReleaseRecordError::Other(
                "it is not in the metadata cache, and the registry may not be reached".into(),
            ));
        }
        let (body, lasting) = self.download().await?;
        let record = self.parse(&body).map_err(ReleaseRecordError::Other)?;
        if let Some(path) = path.filter(|_| lasting) {
            // A record that fails to persist is fetched again next time.
            let _ = save_meta(&path, &body);
        }
        Ok(record)
    }

    /// Where the record is cached, in the same metadata scope as its
    /// package document, so a caller-gated record stays with its caller.
    fn disk_path(&self) -> Option<PathBuf> {
        let cache_dir = self.cache_dir?;
        let auth_headers = self.http.auth_headers;
        let scope = auth_headers.metadata_scope(self.url, Some(self.name));
        let meta_dir = scoped_meta_dir(&scope, RELEASE_RECORDS_DIR);
        let address = (self.version, self.revision);
        release_record_path(cache_dir, &meta_dir, self.registry, self.name, address).ok()
    }

    async fn retry_local_response(
        &self,
        first: LocalResponse,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let http = self.http;
        let mut first = Some(first);
        retry_async(
            self.url,
            http.retry_opts,
            |error: &reqwest::Error| error.status().is_some_and(should_retry_status),
            || {
                let response = first
                    .take()
                    .or_else(|| {
                        http.auth_headers.fetch_local(
                            self.url,
                            Some(self.name),
                            header::HeaderMap::new(),
                        )
                    })
                    .expect("local route remains available during a resolve");
                async move { response.await.error_for_status() }
            },
        )
        .await
    }

    /// The record's body, and whether it may be kept on disk with no
    /// expiry: pnpr serves a record `immutable` once it has derived every
    /// fact about the archive. A caller-gated package's records are
    /// `private, no-store`, so they are read once per install and never
    /// kept.
    async fn download(&self) -> Result<(Vec<u8>, bool), ReleaseRecordError> {
        let local =
            self.http.auth_headers.fetch_local(self.url, Some(self.name), header::HeaderMap::new());
        if let Some(response) = local {
            let response = self.download_local(response).await?;
            return read_release_record_response(response).await;
        }
        let (_guard, response) = self.download_remote().await?;
        read_release_record_response(response).await
    }

    async fn download_local(
        &self,
        response: LocalResponse,
    ) -> Result<reqwest::Response, ReleaseRecordError> {
        self.retry_local_response(response).await
            .map_err(|error| {
                if error.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    ReleaseRecordError::NotFound(error.to_string())
                } else {
                    ReleaseRecordError::Other(error.to_string())
                }
            })
    }

    async fn download_remote(
        &self,
    ) -> Result<(pnpm_network::ThrottledClientGuard<'_>, reqwest::Response), ReleaseRecordError>
    {
        let http = self.http;
        let name = self.name;
        send_with_retry(http.http_client, self.url, http.retry_opts, |client| {
            let request = client.get(self.url).header(header::ACCEPT, "application/json");
            match http.auth_headers.for_url_with_package(self.url, Some(name)) {
                Some(value) => request.header(header::AUTHORIZATION, value),
                None => request,
            }
        })
        .await
        .map_err(|error| ReleaseRecordError::Other(error.to_string()))
    }

    /// The record in `body`, checked to name the picked version.
    fn parse(&self, body: &[u8]) -> Result<Arc<PackageVersion>, String> {
        let record = pnpm_registry::parse_release_record(body).map_err(|error| error.to_string())?;
        if record.name != self.name || record.version.to_string() != self.version {
            return Err(format!("it describes {}@{}", record.name, record.version));
        }
        Ok(Arc::new(record))
    }
}

async fn read_release_record_response(
    response: reqwest::Response,
) -> Result<(Vec<u8>, bool), ReleaseRecordError> {
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        let error = response.error_for_status().expect_err("404 is an error status");
        return Err(ReleaseRecordError::NotFound(error.to_string()));
    }
    let response = response
        .error_for_status()
        .map_err(|error| ReleaseRecordError::Other(error.to_string()))?;
    let lasting = is_immutable_cache_control(response.headers());
    let body = read_limited_body(response, RELEASE_RECORD_BODY_LIMIT).await
        .map_err(|error| ReleaseRecordError::Other(error.to_string()))?;
    if body.truncated {
        return Err(ReleaseRecordError::Other(format!(
            "it is larger than {RELEASE_RECORD_BODY_LIMIT} bytes",
        )));
    }
    Ok((body.bytes, lasting))
}

fn is_immutable_cache_control(headers: &header::HeaderMap) -> bool {
    headers
        .get(header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .map(str::trim)
                .any(|directive| directive == "immutable")
        })
}

/// The picked `version`, read from the release record it needs: the
/// requested revision's when a `+r<N>` override names a revision other than
/// the selected one, else the selected revision's when the document left the
/// version's manifest out. What may change after publication (the
/// deprecation and the trust evidence) always comes from the document.
pub(super) async fn with_release_record<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: Arc<PackageVersion>,
    requested: Option<&RegistryRevisionSelector>,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    if !serves_pnpr_format(ctx, registry) {
        return Ok(version);
    }
    let Some(revision) = record_revision(&version, requested) else {
        return Ok(version);
    };
    let record = release_record(ctx, registry, &version, revision).await?;
    let mut picked = PackageVersion::clone(&record);
    picked.deprecated.clone_from(&version.deprecated);
    picked.npm_user.clone_from(&version.npm_user);
    picked.dist.attestations.clone_from(&version.dist.attestations);
    // The record's address names its revision, whatever the record says: a
    // revision-aware version installs that revision's tar, any other version
    // its canonical tarball.
    picked.dist.revision =
        version.dist.revision.is_some().then(|| serde_json::Value::from(revision));
    Ok(Arc::new(picked))
}

/// The revision whose release record the pick needs, if any. A version
/// outside the revision protocol has only its original, revision 0, so an
/// override naming another one is left for the revision check to reject.
fn record_revision(
    version: &PackageVersion,
    requested: Option<&RegistryRevisionSelector>,
) -> Option<u64> {
    let entry = version.dist.install_v2_entry()?;
    let selected = version.dist.revision.as_ref().and_then(serde_json::Value::as_u64);
    if let Some(RegistryRevisionSelector::Valid(requested)) = requested {
        match selected {
            Some(selected) if selected != *requested => return Some(*requested),
            None if *requested != 0 => return None,
            _ => {}
        }
    }
    (entry == InstallV2Entry::External).then(|| selected.unwrap_or(0))
}

async fn release_record<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: &PackageVersion,
    revision: u64,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    let version_str = version.version.to_string();
    let url = format!("{}/{version_str}+r{revision}", to_registry_url(registry, &version.name));
    let cell = Arc::clone(
        ctx.metadata.fetch_locker.release_records
            .entry(url.clone())
            .or_default()
            .value(),
    );
    let fetch = ReleaseRecordRequest {
        cache_dir: ctx.metadata.cache_dir,
        forbids_registry: ctx.cache_policy.network.forbids_registry(),
        http: ctx.metadata.http,
        registry,
        url: &url,
        name: &version.name,
        version: &version_str,
        revision,
    };
    cell.get_or_try_init(|| async { fetch.run().await.map_err(ReleaseRecordError::into_message) })
        .await
        .cloned()
        .map_err(|error| PickPackageError::ReleaseRecord { url: url.clone(), error })
}

#[cfg(test)]
mod tests;

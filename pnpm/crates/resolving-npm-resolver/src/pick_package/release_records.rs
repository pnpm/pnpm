//! Release records of versions picked from pnpr's own package document
//! (see `pnpm_registry::parse_install_v2`). An entry of that document may give
//! a version only by what picking reads and what may change after
//! publication; the rest of the version, and every revision other than the
//! selected one, is its release record,
//! `<registry><name>/<version>+r<revision>`. A record never changes, so it is
//! fetched once per install, shared between the picks that need it, and
//! cached on disk with no expiry.

use std::{path::PathBuf, sync::Arc};

use pnpm_network::send_with_retry;
use pnpm_registry::{InstallV2Entry, PackageVersion};
use pnpm_resolving_resolver_base::ResolveError;
use reqwest::header;
use tokio::sync::OnceCell;

use super::{PackageMetaCache, PickPackageContext, PickPackageError, serves_pnpr_format};
use crate::{
    RegistryRevisionSelector,
    mirror::{RELEASE_RECORDS_DIR, release_record_path, save_meta, scoped_meta_dir},
    registry_url::to_registry_url,
};

/// The in-flight and fetched records of one install, keyed by URL.
pub(super) type ReleaseRecords = dashmap::DashMap<String, Arc<OnceCell<Arc<PackageVersion>>>>;

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
    if version.dist.revision.is_some() {
        picked.dist.revision = Some(serde_json::Value::from(revision));
    }
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
    if let Some(RegistryRevisionSelector::Valid(requested)) = requested
        && selected.is_some_and(|selected| selected != *requested)
    {
        return Some(*requested);
    }
    (entry == InstallV2Entry::External).then(|| selected.unwrap_or(0))
}

async fn release_record<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    registry: &str,
    version: &PackageVersion,
    revision: u64,
) -> Result<Arc<PackageVersion>, PickPackageError> {
    let url =
        format!("{}/{}+r{revision}", to_registry_url(registry, &version.name), version.version);
    let cell = Arc::clone(
        ctx.metadata.fetch_locker.release_records
            .entry(url.clone())
            .or_default()
            .value(),
    );
    let fetch = RecordFetch { ctx, registry, url: &url, version, revision };
    cell.get_or_try_init(|| fetch.run()).await
        .cloned()
        .map_err(|error| PickPackageError::ReleaseRecord { url: url.clone(), error })
}

/// One release record to read, from disk or from the registry.
struct RecordFetch<'ctx, Cache: PackageMetaCache> {
    ctx: &'ctx PickPackageContext<'ctx, Cache>,
    registry: &'ctx str,
    url: &'ctx str,
    version: &'ctx PackageVersion,
    revision: u64,
}

impl<Cache: PackageMetaCache> RecordFetch<'_, Cache> {
    async fn run(&self) -> Result<Arc<PackageVersion>, String> {
        let path = self.disk_path();
        if let Some(record) = path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|body| self.parse(&body).ok())
        {
            return Ok(record);
        }
        if self.ctx.cache_policy.network.forbids_registry() {
            return Err(
                "it is not in the metadata cache, and the registry may not be reached".into()
            );
        }
        let body = self.download().await.map_err(|error| error.to_string())?;
        let record = self.parse(&body)?;
        if let Some(path) = path {
            // A record that fails to persist is fetched again next time.
            let _ = save_meta(&path, &body);
        }
        Ok(record)
    }

    /// Where the record is cached, in the same metadata scope as its
    /// package document, so a caller-gated record stays with its caller.
    fn disk_path(&self) -> Option<PathBuf> {
        let cache_dir = self.ctx.metadata.cache_dir?;
        let auth_headers = self.ctx.metadata.http.auth_headers;
        let scope = auth_headers.metadata_scope(self.url, Some(&self.version.name));
        let meta_dir = scoped_meta_dir(&scope, RELEASE_RECORDS_DIR);
        let version = self.version.version.to_string();
        let address = (version.as_str(), self.revision);
        release_record_path(cache_dir, &meta_dir, self.registry, &self.version.name, address).ok()
    }

    async fn download(&self) -> Result<Vec<u8>, ResolveError> {
        let http = self.ctx.metadata.http;
        let name = &self.version.name;
        let (_guard, response) =
            send_with_retry(http.http_client, self.url, http.retry_opts, |client| {
                let request = client.get(self.url).header(header::ACCEPT, "application/json");
                match http.auth_headers.for_url_with_package(self.url, Some(name)) {
                    Some(value) => request.header(header::AUTHORIZATION, value),
                    None => request,
                }
            })
            .await?;
        Ok(response
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }

    /// The record in `body`, checked to name the picked version.
    fn parse(&self, body: &[u8]) -> Result<Arc<PackageVersion>, String> {
        let record = pnpm_registry::parse_release_record(body).map_err(|error| error.to_string())?;
        if record.name != self.version.name || record.version != self.version.version {
            return Err(format!("it describes {}@{}", record.name, record.version));
        }
        Ok(Arc::new(record))
    }
}

#[cfg(test)]
mod tests;

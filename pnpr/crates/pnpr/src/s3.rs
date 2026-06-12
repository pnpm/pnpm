//! S3-compatible object-store backend for the **hosted** store.
//!
//! The hosted store is pnpr's source of truth — packages published
//! through its API plus the content served in static mode. When the
//! YAML `s3:` block is present, those authoritative packuments and
//! tarballs live in an object store instead of on local disk, so the
//! durable data can be replicated by the provider and shared by
//! several stateless pnpr replicas.
//!
//! Any S3-compatible endpoint works: AWS S3 (omit `endpoint`),
//! Cloudflare R2 (`region: auto`, the account endpoint), `MinIO`,
//! Backblaze B2, Wasabi, etc. The disposable proxy cache and the
//! resolver `SQLite` stores stay on local disk regardless —
//! only the hosted store is pluggable.

use crate::{error::Result, package_name::PackageName};
use axum::body::Body;
use futures_util::StreamExt;
use object_store::{ObjectStore, PutPayload, aws::AmazonS3Builder, path::Path as ObjectPath};
use serde::Deserialize;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;

const PACKUMENT_FILE: &str = "package.json";

/// The YAML `s3:` block. Selects the object-store hosted backend.
/// Credentials fall back to the standard AWS environment variables
/// (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`) when not set here,
/// so an operator can keep secrets out of the config file. Whole-file
/// `${ENV}` substitution still runs first, so inline `${...}` values
/// work too.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Settings {
    /// Bucket the hosted packages live in.
    pub bucket: String,
    /// Region. AWS S3 needs a real region; Cloudflare R2 uses `auto`.
    #[serde(default)]
    pub region: Option<String>,
    /// Custom endpoint for S3-compatible providers. Omit for AWS S3;
    /// for R2 this is `https://<account-id>.r2.cloudflarestorage.com`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Key prefix every object is stored under (e.g. `packages`).
    /// Lets one bucket hold more than just the hosted store.
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    /// Force path-style addressing (`endpoint/bucket/key`) instead of
    /// virtual-hosted (`bucket.endpoint/key`). `MinIO` typically needs
    /// this; AWS and R2 work with the default.
    #[serde(default)]
    pub force_path_style: Option<bool>,
    /// Allow plain-HTTP endpoints — needed for a local `MinIO` over
    /// `http://`. Defaults to HTTPS-only.
    #[serde(default)]
    pub allow_http: Option<bool>,
}

impl S3Settings {
    /// The configured key prefix, normalized to either `""` or a value
    /// ending in `/` so it can be string-concatenated onto object keys.
    pub fn normalized_prefix(&self) -> String {
        match self.prefix.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
            None => String::new(),
            Some(prefix) => {
                let trimmed = prefix.trim_matches('/');
                if trimmed.is_empty() { String::new() } else { format!("{trimmed}/") }
            }
        }
    }
}

/// Build the object-store client from the YAML `s3:` settings. The
/// builder seeds from the AWS environment first so env credentials
/// work out of the box, then the explicit YAML values override.
/// Failures here are config errors surfaced at startup, not over HTTP.
pub fn build_s3_store(settings: &S3Settings) -> Result<Arc<dyn ObjectStore>> {
    let mut builder = AmazonS3Builder::from_env().with_bucket_name(&settings.bucket);
    if let Some(region) = &settings.region {
        builder = builder.with_region(region);
    }
    if let Some(endpoint) = &settings.endpoint {
        builder = builder.with_endpoint(endpoint);
    }
    if let Some(key) = &settings.access_key_id {
        builder = builder.with_access_key_id(key);
    }
    if let Some(secret) = &settings.secret_access_key {
        builder = builder.with_secret_access_key(secret);
    }
    if let Some(force_path_style) = settings.force_path_style {
        builder = builder.with_virtual_hosted_style_request(!force_path_style);
    }
    if let Some(allow_http) = settings.allow_http {
        builder = builder.with_allow_http(allow_http);
    }
    let store = builder.build().map_err(|err| crate::error::RegistryError::InvalidConfig {
        reason: format!("invalid s3 config: {err}"),
    })?;
    Ok(Arc::new(store))
}

/// Object-store-backed hosted store. Mirrors the verdaccio-shaped
/// key layout the on-disk [`crate::storage`] uses
/// (`<prefix><pkg>/package.json`, `<prefix><pkg>/<basename>.tgz`) so a
/// bucket and a directory hold the same shape.
#[derive(Debug, Clone)]
pub struct S3Store {
    store: Arc<dyn ObjectStore>,
    /// Normalized prefix: empty or `.../`-terminated.
    prefix: String,
    /// Local directory the publish flow stages decoded tarballs in
    /// before they're uploaded. The decode/verify step writes through
    /// `std::fs` inside `spawn_blocking`, so it needs a real path even
    /// when the final home is a bucket; a subdirectory of the
    /// proxy-cache root doubles as scratch.
    staging_dir: PathBuf,
}

/// Subdirectory of the proxy-cache root where hosted tarballs are
/// staged before upload. Its own directory keeps the decode/verify tmp
/// files away from the cache's `<pkg>/` package directories.
const STAGING_SUBDIR: &str = "pnpr-hosted-staging";

impl S3Store {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "constructor; cache_root seeds staging_dir without threading &Path through storage::new and its construction sites"
    )]
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String, cache_root: PathBuf) -> Self {
        Self { store, prefix, staging_dir: cache_root.join(STAGING_SUBDIR) }
    }

    pub async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.packument_key(name)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn write_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        self.store.put(&self.packument_key(name), PutPayload::from(bytes.to_vec())).await?;
        Ok(())
    }

    /// Open a hosted tarball for streaming. `Ok(None)` means the object
    /// doesn't exist so the caller can fall through to the proxy cache
    /// or upstream.
    pub async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        match self.store.get(&self.tarball_key(name, filename)).await {
            Ok(result) => {
                let len = result.meta.size;
                let stream = result
                    .into_stream()
                    .map(|chunk| chunk.map_err(|err| io::Error::other(err.to_string())));
                Ok(Some((Body::from_stream(stream), Some(len))))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Reserve a local staging path for the publish flow to decode and
    /// verify a tarball into; [`Self::upload_tarball`] promotes it to
    /// the bucket once the verification passes.
    pub async fn staging_tmp_path(&self, _name: &PackageName, filename: &str) -> Result<PathBuf> {
        fs::create_dir_all(&self.staging_dir).await?;
        Ok(crate::storage::unique_tmp_path(&self.staging_dir.join(filename)))
    }

    pub async fn upload_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<()> {
        let bytes = fs::read(tmp_path).await?;
        self.store.put(&self.tarball_key(name, filename), PutPayload::from(bytes)).await?;
        Ok(())
    }

    pub async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        match self.store.delete(&self.tarball_key(name, filename)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        let prefix = ObjectPath::from(format!("{}{}/", self.prefix, name.as_str()));
        let mut listing = self.store.list(Some(&prefix));
        let mut removed = false;
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            self.store.delete(&meta.location).await?;
            removed = true;
        }
        Ok(removed)
    }

    /// List the hosted package names (verdaccio-shaped: a name is a
    /// directory holding a `package.json`). Backs the local search
    /// endpoint when the hosted store lives in a bucket.
    pub async fn list_package_names(&self) -> Result<Vec<String>> {
        let scope = (!self.prefix.is_empty())
            .then(|| ObjectPath::from(self.prefix.trim_end_matches('/').to_string()));
        let mut listing = self.store.list(scope.as_ref());
        let mut names = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let key = meta.location.as_ref();
            // Skip anything that isn't actually under our prefix rather
            // than falling back to the full key, which would synthesize
            // a wrong name. (Empty prefix strips to the whole key.)
            let Some(rest) = key.strip_prefix(self.prefix.as_str()) else {
                continue;
            };
            if let Some(name) = rest.strip_suffix(&format!("/{PACKUMENT_FILE}")) {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    fn packument_key(&self, name: &PackageName) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{PACKUMENT_FILE}", self.prefix, name.as_str()))
    }

    fn tarball_key(&self, name: &PackageName, filename: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{filename}", self.prefix, name.as_str()))
    }
}

#[cfg(test)]
mod tests;

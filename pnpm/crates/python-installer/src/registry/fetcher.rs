use super::{CachedIndex, MAX_CACHE_BYTES, MAX_INDEX_BYTES, read_cached_index};
use crate::Index;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_python_resolver::{IndexCandidate, WheelMetadata, matches_published_digest, metadata_url};
use std::{collections::BTreeMap, path::Path};
use tokio::io::AsyncReadExt;
use url::Url;

const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

/// What a resolution reads from the index. It borrows nothing the
/// resolution records into, so reads run while the resolution goes on.
#[derive(Clone, Copy)]
pub(in super::super) struct Fetcher<'a> {
    pub(super) config: &'static Config,
    pub(super) client: &'a ThrottledClient,
    pub(super) index: &'a Index,
}

impl Fetcher<'_> {
    /// The Simple JSON index page for `name`: from the cache when
    /// `replayed`, and from the index otherwise.
    pub(in super::super) async fn index_page(
        self,
        name: &PackageName,
        replayed: bool,
    ) -> Result<CachedIndex> {
        let index_url = self.index
            .select(name.as_ref())?
            .join(&format!("{name}/"))
            .into_diagnostic()?;
        let cache = self.config.cache_dir
            .join("python-index-v3")
            .join(format!("{}.json", self.index.cache_key(&index_url)));
        let cached = if replayed {
            read_cached_index(&cache, name).await?
        } else {
            self.download_index(&index_url, name).await?
        };
        if cached.body.get().len() > MAX_INDEX_BYTES {
            bail!("Python index response for {name} exceeds {MAX_INDEX_BYTES} bytes");
        }
        if !replayed {
            tokio::fs::create_dir_all(cache.parent().expect("cache file has a parent"))
                .await
                .into_diagnostic()?;
            let contents = serde_json::to_vec(&cached).into_diagnostic()?;
            if contents.len() > MAX_CACHE_BYTES {
                bail!("Python index cache for {name} exceeds {MAX_CACHE_BYTES} bytes");
            }
            pnpm_fs::write_atomic(&cache, &contents).into_diagnostic()?;
        }
        Ok(cached)
    }

    async fn download_index(self, index_url: &Url, name: &PackageName) -> Result<CachedIndex> {
        let response = self.client
            .get_limited_bytes_with_secure_auth_and_retry(
                index_url.as_str(),
                &self.index.auth,
                Some("application/vnd.pypi.simple.v1+json"),
                self.config.retry_opts(),
                MAX_INDEX_BYTES,
            )
            .await
            .into_diagnostic()?;
        CachedIndex::from_response(name, &response)
    }

    /// The `METADATA` the index publishes beside the wheel of `candidate`
    /// (PEP 658), or `None` when there is none to read: the index
    /// publishes no such file or no SHA-256 to check it against, or an
    /// offline resolution has not cached it. The caller then reads the
    /// metadata out of the wheel.
    pub(in super::super) async fn core_metadata(
        self,
        name: &PackageName,
        version: &Version,
        candidate: &IndexCandidate,
    ) -> Result<Option<WheelMetadata>> {
        let Some(digests) = checked_metadata_digests(candidate) else { return Ok(None) };
        let filename = &candidate.wheel.name;
        let url = metadata_url(&Url::parse(&candidate.wheel.url).into_diagnostic()?);
        let cache = self.config.cache_dir
            .join("python-metadata-v1")
            .join(self.index.cache_key(&url));
        let document = match read_cached_metadata(&cache).await? {
            Some(document) if matches_published_digest(&document, digests) => document,
            _ if self.config.offline => return Ok(None),
            _ => {
                let Some(document) = self.download_metadata(&url, filename).await? else {
                    return Ok(None);
                };
                if !matches_published_digest(&document, digests) {
                    bail!(
                        "the metadata file of {filename} does not match the SHA-256 the index \
                         published",
                    );
                }
                tokio::fs::create_dir_all(cache.parent().expect("cache file has a parent"))
                    .await
                    .into_diagnostic()?;
                pnpm_fs::write_atomic(&cache, &document).into_diagnostic()?;
                document
            }
        };
        let document = String::from_utf8(document)
            .into_diagnostic()
            .wrap_err_with(|| format!("decode the metadata file of {filename}"))?;
        WheelMetadata::parse_for(&document, name, version, filename).map(Some)
    }

    /// The metadata file at `url`, or `None` when the index does not
    /// serve the file its page said it publishes.
    async fn download_metadata(self, url: &Url, filename: &str) -> Result<Option<Vec<u8>>> {
        let response = self.client
            .get_limited_bytes_with_secure_auth_and_retry(
                url.as_str(),
                &self.index.auth,
                None,
                self.config.retry_opts(),
                MAX_METADATA_BYTES,
            )
            .await
            .into_diagnostic()?;
        if response.status.as_u16() == 404 {
            return Ok(None);
        }
        if !response.status.is_success() {
            bail!("the metadata file of {filename} returned {}", response.status);
        }
        if response.body_truncated {
            bail!("the metadata file of {filename} exceeds {MAX_METADATA_BYTES} bytes");
        }
        Ok(Some(response.body))
    }
}

/// The digests of the metadata file the index publishes beside the wheel
/// of `candidate`, when they include a SHA-256 to check the file against.
pub(in super::super) fn checked_metadata_digests(
    candidate: &IndexCandidate,
) -> Option<&BTreeMap<String, String>> {
    candidate.core_metadata
        .as_ref()
        .filter(|digests| digests.contains_key("sha256"))
}

/// A metadata file an earlier run cached, or `None` when none is cached
/// whole.
async fn read_cached_metadata(cache: &Path) -> Result<Option<Vec<u8>>> {
    let file = match tokio::fs::File::open(cache).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };
    let mut contents = Vec::new();
    file.take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut contents)
        .await
        .into_diagnostic()?;
    Ok((contents.len() <= MAX_METADATA_BYTES).then_some(contents))
}

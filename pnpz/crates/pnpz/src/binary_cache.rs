use derive_more::{
    Display,
    Error,
};
use futures_util::StreamExt as _;
use miette::Diagnostic;
use pnpm_network::ThrottledClient;
use pnpz_nix_store::{
    NarInfo,
    ParseNarInfoError,
    SignatureError,
    StorePath,
    TrustedKey,
    nar::UnpackError,
};
use reqwest::StatusCode;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        Mutex,
    },
};
use tokio::sync::mpsc;

mod download;

pub const DEFAULT_CACHE_URL: &str = "https://cache.nixos.org";

/// Overrides [`DEFAULT_CACHE_URL`].
pub const CACHE_URL_ENV: &str = "PNPZ_CACHE_URL";

/// Space-separated `<name>:<base64 key>` entries that replace the default
/// trusted key, the one of `cache.nixos.org`.
pub const TRUSTED_KEYS_ENV: &str = "PNPZ_TRUSTED_PUBLIC_KEYS";

/// How many compressed chunks may wait for the thread that unpacks them.
const CHUNK_BACKLOG: usize = 16;

/// A Nix binary cache, read over HTTP.
pub struct BinaryCache {
    client: Arc<ThrottledClient>,
    base_url: String,
    trusted_keys: Vec<TrustedKey>,
    /// Resolving a closure and installing it both need every path's narinfo.
    narinfos: Mutex<HashMap<StorePath, Arc<NarInfo>>>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum CacheError {
    #[display("{store_path} is not in the binary cache at {cache}")]
    #[diagnostic(code(ERR_PNPZ_NOT_IN_CACHE))]
    NotFound { store_path: String, cache: String },
    #[display("the request for {url} failed: {source}")]
    #[diagnostic(code(ERR_PNPZ_CACHE_REQUEST))]
    Request { url: String, source: reqwest::Error },
    #[display("the binary cache answered {status} for {url}")]
    #[diagnostic(code(ERR_PNPZ_CACHE_STATUS))]
    Status { url: String, status: StatusCode },
    #[display("{url}: {source}")]
    #[diagnostic(code(ERR_PNPZ_INVALID_NARINFO))]
    InvalidNarInfo { url: String, source: ParseNarInfoError },
    #[display("{url} describes {found}, not {expected}")]
    #[diagnostic(code(ERR_PNPZ_NARINFO_MISMATCH))]
    WrongStorePath { url: String, expected: String, found: String },
    #[diagnostic(transparent)]
    Untrusted(SignatureError),
    #[display("failed to unpack {store_path}: {source}")]
    #[diagnostic(code(ERR_PNPZ_UNPACK))]
    Unpack { store_path: String, source: UnpackError },
    #[display("the archive of {store_path} is {found}, and its signed description says {expected}")]
    #[diagnostic(code(ERR_PNPZ_NAR_HASH_MISMATCH))]
    HashMismatch { store_path: String, expected: String, found: String },
}

impl BinaryCache {
    pub fn new(
        client: Arc<ThrottledClient>,
        base_url: &str,
        trusted_keys: Vec<TrustedKey>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            trusted_keys,
            narinfos: Mutex::default(),
        }
    }

    /// Fetches the description of `path` and checks that a trusted key signed
    /// it.
    pub async fn narinfo(&self, path: &StorePath) -> Result<Arc<NarInfo>, CacheError> {
        if let Some(known) = self.narinfos
            .lock()
            .expect("narinfo cache lock")
            .get(path)
        {
            return Ok(Arc::clone(known));
        }
        let url = format!("{}/{}.narinfo", self.base_url, path.hash_part());
        let response = match self.get(&url).await {
            Err(CacheError::Status { status: StatusCode::NOT_FOUND, .. }) => {
                let (store_path, cache) = (path.to_string(), self.base_url.clone());
                return Err(CacheError::NotFound { store_path, cache });
            }
            response => response?,
        };
        let text = match response.text().await {
            Ok(text) => text,
            Err(source) => return Err(CacheError::Request { url, source }),
        };
        let narinfo = self.checked_narinfo(&text, path, url)?;
        let narinfo = Arc::new(narinfo);
        self.narinfos
            .lock()
            .expect("narinfo cache lock")
            .insert(path.clone(), Arc::clone(&narinfo));
        Ok(narinfo)
    }

    fn checked_narinfo(
        &self,
        text: &str,
        path: &StorePath,
        url: String,
    ) -> Result<NarInfo, CacheError> {
        let narinfo = match NarInfo::parse(text) {
            Ok(narinfo) => narinfo,
            Err(source) => return Err(CacheError::InvalidNarInfo { url, source }),
        };
        if narinfo.store_path != *path {
            let (expected, found) = (path.to_string(), narinfo.store_path.to_string());
            return Err(CacheError::WrongStorePath { url, expected, found });
        }
        narinfo.verify(&self.trusted_keys).map_err(CacheError::Untrusted)?;
        Ok(narinfo)
    }

    /// Downloads the archive `narinfo` describes and unpacks it at
    /// `destination`, which must not exist.
    ///
    /// The archive is unpacked while it downloads and checked against the
    /// signed hash and size at the end. On any failure `destination` is
    /// removed.
    pub async fn fetch(&self, narinfo: &NarInfo, destination: &Path) -> Result<(), CacheError> {
        let url = format!("{}/{}", self.base_url, narinfo.url);
        let response = self.get(&url).await?;
        let (sender, receiver) = mpsc::channel(CHUNK_BACKLOG);
        let unpacking = tokio::task::spawn_blocking({
            let (narinfo, destination) = (narinfo.clone(), destination.to_path_buf());
            move || download::unpack_verified(receiver, &narinfo, &destination)
        });
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            if sender
                .send(chunk.map_err(std::io::Error::other))
                .await
                .is_err()
            {
                break;
            }
        }
        drop(sender);
        unpacking.await.expect("the unpacking thread does not panic")
    }

    async fn get(&self, url: &str) -> Result<reqwest::Response, CacheError> {
        let client = self.client.acquire_for_url(url).await;
        let response = match client.get(url).send().await {
            Ok(response) => response,
            Err(source) => return Err(CacheError::Request { url: url.to_string(), source }),
        };
        match response.status() {
            StatusCode::OK => Ok(response),
            status => Err(CacheError::Status { url: url.to_string(), status }),
        }
    }
}

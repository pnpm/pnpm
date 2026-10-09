//! The remote tier of the task cache, spoken over the Turborepo Remote Cache
//! API: `GET` and `PUT` of `/v8/artifacts/<key>`. An artifact is one local
//! cache entry as a gzipped tar, signed the way Turborepo signs
//! `x-artifact-tag`: HMAC-SHA256 over the key, the team, and the body.
//!
//! The remote tier only ever adds entries to the local one. A downloaded
//! entry is restored through the same checks as a local entry.

use super::cache::{StoredTask, TaskCache};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use hmac::{Hmac, Mac};
use pnpm_config::Config;
use pnpm_network::{ThrottledClient, is_url_secure_for_credentials, read_limited_body};
use reqwest::{RequestBuilder, StatusCode};
use sha2::Sha256;
use std::{
    fs, io,
    io::Read,
    path::{Component, Path},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{runtime::Handle, task::JoinHandle};

const ARTIFACT_TAG_HEADER: &str = "x-artifact-tag";
const ARTIFACT_DURATION_HEADER: &str = "x-artifact-duration";
const MAX_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(super) struct RemoteTaskCache {
    client: Arc<ThrottledClient>,
    artifacts_url: String,
    /// `?teamId=…` or `?slug=…`, or empty.
    team_query: String,
    authorization: Option<String>,
    signer: ArtifactSigner,
    upload: bool,
    runtime: Handle,
    uploads: Mutex<Vec<JoinHandle<Result<(), String>>>>,
}

impl RemoteTaskCache {
    /// The remote tier `pipelineRemoteCache` configures. `Ok(None)` when it
    /// names no server, `Err` with the reason when it names one that cannot
    /// be used.
    pub(super) fn open(config: &Config) -> Result<Option<RemoteTaskCache>, String> {
        let Some(settings) = &config.pipeline_remote_cache else {
            return Ok(None);
        };
        let Some(url) = settings.url.as_deref() else {
            return Ok(None);
        };
        if !is_url_secure_for_credentials(url) {
            return Err(format!("{url} is neither HTTPS nor a loopback address"));
        }
        let Some(signature_key) = settings.signature_key
            .as_deref()
            .filter(|key| !key.is_empty())
        else {
            return Err("pipelineRemoteCache.signatureKey is not set".to_string());
        };
        let client = ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .map_err(|error| error.to_string())?;
        let runtime = Handle::try_current().map_err(|error| error.to_string())?;
        let team = settings.team.clone().unwrap_or_default();
        Ok(Some(RemoteTaskCache {
            client: Arc::new(client),
            artifacts_url: format!("{}/v8/artifacts", url.trim_end_matches('/')),
            team_query: team_query(&team),
            authorization: settings.token
                .as_ref()
                .map(|token| format!("Bearer {token}"))
                .or_else(|| config.auth_headers.for_secure_url(url)),
            signer: ArtifactSigner { key: signature_key.as_bytes().to_vec(), team },
            upload: settings.upload == Some(true),
            runtime,
            uploads: Mutex::new(Vec::new()),
        }))
    }

    /// Download the artifact stored under `key` into `cache`. `Ok(false)`
    /// when the server has none.
    ///
    /// Blocks, so it must run off the async runtime's worker threads.
    pub(super) fn fetch(&self, key: &str, cache: &TaskCache) -> Result<bool, String> {
        let Some(body) = self.runtime.block_on(self.download(key))? else {
            return Ok(false);
        };
        cache
            .import(key, |staging| unpack(&body, staging))
            .map_err(|error| format!("unpacking the artifact: {error}"))?;
        Ok(true)
    }

    async fn download(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let url = self.artifact_url(key);
        let client = self.client.acquire_for_url(&url).await;
        let response = self
            .authorize(client.get(&url))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!("the server answered {}", response.status()));
        }
        let tag = response
            .headers()
            .get(ARTIFACT_TAG_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = read_limited_body(response, MAX_ARTIFACT_BYTES).await
            .map_err(|error| error.to_string())?;
        if body.truncated {
            return Err(format!("the artifact is larger than {MAX_ARTIFACT_BYTES} bytes"));
        }
        self.signer.verify(key, &body.bytes, tag.as_deref())?;
        Ok(Some(body.bytes))
    }

    /// Start uploading `stored` under `key` when uploads are on. The upload
    /// runs in the background; [`Self::finish_uploads`] waits for it.
    pub(super) fn upload(
        &self,
        key: &str,
        stored: &StoredTask,
        duration: Duration,
    ) -> Result<(), String> {
        if !self.upload {
            return Ok(());
        }
        let body = pack(stored).map_err(|error| format!("packing the artifact: {error}"))?;
        let request = PendingUpload {
            url: self.artifact_url(key),
            tag: self.signer.sign(key, &body),
            authorization: self.authorization.clone(),
            duration_ms: duration.as_millis().to_string(),
            body,
        };
        let client = Arc::clone(&self.client);
        let upload = self.runtime.spawn(async move { request.send(&client).await });
        self.uploads
            .lock()
            .expect("upload list lock is not poisoned")
            .push(upload);
        Ok(())
    }

    /// Wait for every upload [`Self::upload`] started, returning why each
    /// failed one did.
    pub(super) fn finish_uploads(&self) -> Vec<String> {
        let uploads =
            std::mem::take(&mut *self.uploads.lock().expect("upload list lock is not poisoned"));
        if uploads.is_empty() {
            return Vec::new();
        }
        // The caller may be a worker of the runtime the uploads run on,
        // which cannot block on it.
        std::thread::scope(|scope| {
            scope
                .spawn(|| self.wait_for(uploads))
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        })
    }

    fn wait_for(&self, uploads: Vec<JoinHandle<Result<(), String>>>) -> Vec<String> {
        self.runtime.block_on(async {
            let mut failures = Vec::new();
            for upload in uploads {
                match upload.await {
                    Ok(Ok(())) => {}
                    Ok(Err(reason)) => failures.push(reason),
                    Err(error) => failures.push(error.to_string()),
                }
            }
            failures
        })
    }

    fn artifact_url(&self, key: &str) -> String {
        format!("{}/{key}{}", self.artifacts_url, self.team_query)
    }

    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.authorization {
            Some(authorization) => request.header("authorization", authorization),
            None => request,
        }
    }
}

/// The `x-artifact-tag` of an artifact: base64 HMAC-SHA256 over the key, the
/// team, and the body, keyed by `pipelineRemoteCache.signatureKey`.
struct ArtifactSigner {
    key: Vec<u8>,
    team: String,
}

impl ArtifactSigner {
    fn mac(&self, cache_key: &str, body: &[u8]) -> Hmac<Sha256> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts keys of any length");
        mac.update(cache_key.as_bytes());
        mac.update(self.team.as_bytes());
        mac.update(body);
        mac
    }

    fn sign(&self, cache_key: &str, body: &[u8]) -> String {
        BASE64.encode(
            self.mac(cache_key, body)
                .finalize()
                .into_bytes(),
        )
    }

    fn verify(&self, cache_key: &str, body: &[u8], tag: Option<&str>) -> Result<(), String> {
        let tag = tag.ok_or("the artifact is not signed")?;
        let tag = BASE64.decode(tag).map_err(|_| "the artifact signature is malformed")?;
        self.mac(cache_key, body)
            .verify_slice(&tag)
            .map_err(|_| "the artifact signature does not match".to_string())
    }
}

/// Turborepo's convention: a Vercel team id starts with `team_`, anything
/// else is a team slug.
fn team_query(team: &str) -> String {
    if team.is_empty() {
        return String::new();
    }
    let name = if team.starts_with("team_") { "teamId" } else { "slug" };
    let query =
        url::form_urlencoded::Serializer::new(String::new()).append_pair(name, team).finish();
    format!("?{query}")
}

struct PendingUpload {
    url: String,
    tag: String,
    authorization: Option<String>,
    duration_ms: String,
    body: Vec<u8>,
}

impl PendingUpload {
    async fn send(self, client: &ThrottledClient) -> Result<(), String> {
        let guard = client.acquire_for_url(&self.url).await;
        let mut request = guard
            .put(&self.url)
            .header("content-type", "application/octet-stream")
            .header(ARTIFACT_TAG_HEADER, &self.tag)
            .header(ARTIFACT_DURATION_HEADER, &self.duration_ms);
        if let Some(authorization) = &self.authorization {
            request = request.header("authorization", authorization);
        }
        let response = request
            .body(self.body)
            .send()
            .await
            .map_err(|error| format!("{}: {error}", self.url))?;
        if !response.status().is_success() {
            return Err(format!("{}: the server answered {}", self.url, response.status()));
        }
        Ok(())
    }
}

/// `meta.json` and the `outputs/` tree of a local entry, as a gzipped tar.
fn pack(stored: &StoredTask) -> io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    builder.mode(tar::HeaderMode::Deterministic);
    builder.follow_symlinks(false);
    builder.append_path_with_name(stored.entry_dir.join("meta.json"), "meta.json")?;
    for relative in &stored.files {
        builder.append_path_with_name(
            stored.entry_dir.join("outputs").join(relative),
            format!("outputs/{relative}"),
        )?;
    }
    builder.into_inner()?.finish()
}

/// Unpack an artifact into an empty `staging` directory. Only `meta.json`
/// and regular files and directories under `outputs/` are accepted, so
/// nothing an artifact carries can write outside `staging`.
fn unpack(archive: &[u8], staging: &Path) -> io::Result<()> {
    let mut archive = tar::Archive::new(GzDecoder::new(archive).take(MAX_UNPACKED_BYTES));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if !is_entry_path(&path) {
            return Err(io::Error::other(format!("unexpected path {}", path.display())));
        }
        let target = staging.join(&path);
        match entry.header().entry_type() {
            tar::EntryType::Directory => fs::create_dir_all(&target)?,
            tar::EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                io::copy(&mut entry, &mut fs::File::create_new(&target)?)?;
            }
            other => {
                return Err(io::Error::other(format!(
                    "unexpected {other:?} entry {}",
                    path.display(),
                )));
            }
        }
    }
    Ok(())
}

fn is_entry_path(path: &Path) -> bool {
    let mut components = path.components();
    let first = components.next();
    let rest_is_normal = components
        .clone()
        .all(|component| matches!(component, Component::Normal(_)));
    match first {
        Some(Component::Normal(name)) if name == "meta.json" => components.next().is_none(),
        Some(Component::Normal(name)) if name == "outputs" => rest_is_normal,
        _ => false,
    }
}

#[cfg(test)]
mod tests;

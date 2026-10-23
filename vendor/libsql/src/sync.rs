use crate::{local::Connection, util::ConnectorService, Error, Result};

use crate::database::EncryptionContext;
use bytes::Bytes;
use chrono::Utc;
use http::{HeaderValue, StatusCode};
use hyper::Body;
use std::path::Path;
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;
use zerocopy::big_endian;

#[cfg(test)]
mod test;

pub mod connection;
pub mod statement;
pub mod transaction;

const METADATA_VERSION: u32 = 0;

const DEFAULT_MAX_RETRIES: usize = 5;
const DEFAULT_PUSH_BATCH_SIZE: u32 = 128;
const DEFAULT_PULL_BATCH_SIZE: u32 = 128;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum SyncError {
    #[error("io: msg={msg}, err={err}")]
    Io {
        msg: &'static str,
        #[source]
        err: std::io::Error,
    },
    #[error("invalid auth header: {0}")]
    InvalidAuthHeader(http::header::InvalidHeaderValue),
    #[error("http dispatch error: {0}")]
    HttpDispatch(hyper::Error),
    #[error("body error: {0}")]
    HttpBody(hyper::Error),
    #[error("json decode error: {0}")]
    JsonDecode(serde_json::Error),
    #[error("json value error, unexpected value: {0}")]
    JsonValue(serde_json::Value),
    #[error("json encode error: {0}")]
    JsonEncode(serde_json::Error),
    #[error("failed to push frame: status={0}, error={1}")]
    PushFrame(StatusCode, String),
    #[error("failed to verify metadata file version: expected={0}, got={1}")]
    VerifyVersion(u32, u32),
    #[error("failed to verify metadata file hash: expected={0}, got={1}")]
    VerifyHash(u32, u32),
    #[error("server returned a lower frame_no: sent={0}, got={1}")]
    InvalidPushFrameNoLow(u32, u32),
    #[error("server returned a higher frame_no: sent={0}, got={1}")]
    InvalidPushFrameNoHigh(u32, u32),
    #[error("server returned a conflict: sent={0}, got={1}")]
    InvalidPushFrameConflict(u32, u32),
    #[error("failed to pull frame: status={0}, error={1}")]
    PullFrame(StatusCode, String),
    #[error("failed to get location header for redirect: {0}")]
    RedirectHeader(http::header::ToStrError),
    #[error("redirect response with no location header")]
    NoRedirectLocationHeader,
    #[error("failed to pull db export: status={0}, error={1}")]
    PullDb(StatusCode, String),
    #[error("server returned a lower generation than local: local={0}, remote={1}")]
    InvalidLocalGeneration(u32, u32),
    #[error("invalid local state: {0}")]
    InvalidLocalState(String),
    #[error("invalid remote state: {0}")]
    InvalidRemoteState(String),
    #[error("server returned invalid length of frames: {0}")]
    InvalidPullFrameBytes(usize),
}

impl SyncError {
    fn io(msg: &'static str) -> impl FnOnce(std::io::Error) -> SyncError {
        move |err| SyncError::Io { msg, err }
    }
}

pub struct PushResult {
    status: PushStatus,
    generation: u32,
    max_frame_no: u32,
    baton: Option<String>,
}

pub struct DropAbort(pub Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for DropAbort {
    fn drop(&mut self) {
        tracing::debug!("aborting");
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

pub enum PushStatus {
    Ok,
    Conflict,
}

pub enum PullResult {
    /// Frames were successfully pulled.
    Frames(Bytes),
    /// We've reached the end of the generation.
    EndOfGeneration { max_generation: u32 },
}

#[derive(serde::Deserialize)]
struct InfoResult {
    current_generation: u32,
}

#[derive(Debug)]
struct PushFramesResult {
    max_frame_no: u32,
    baton: Option<String>,
}

pub struct SyncContext {
    db_path: String,
    client: hyper::Client<ConnectorService, Body>,
    sync_url: String,
    auth_token: Option<HeaderValue>,
    max_retries: usize,
    push_batch_size: u32,
    pull_batch_size: u32,
    /// The current durable generation.
    durable_generation: u32,
    /// Represents the max_frame_no from the server.
    durable_frame_num: u32,
    /// whenever sync is called very first time, we will call the remote server
    /// to get the generation information and sync the db file if needed
    initial_server_sync: bool,
    /// The encryption context for the sync.
    remote_encryption: Option<EncryptionContext>,
}

impl SyncContext {
    pub async fn new(
        connector: ConnectorService,
        db_path: String,
        sync_url: String,
        auth_token: Option<String>,
        remote_encryption: Option<EncryptionContext>,
    ) -> Result<Self> {
        let client = hyper::client::Client::builder().build::<_, hyper::Body>(connector);

        let auth_token = match auth_token {
            Some(t) => Some(
                HeaderValue::try_from(format!("Bearer {}", t))
                    .map_err(SyncError::InvalidAuthHeader)?,
            ),
            None => None,
        };

        let mut me = Self {
            db_path,
            sync_url,
            auth_token,
            max_retries: DEFAULT_MAX_RETRIES,
            push_batch_size: DEFAULT_PUSH_BATCH_SIZE,
            pull_batch_size: DEFAULT_PULL_BATCH_SIZE,
            client,
            durable_generation: 0,
            durable_frame_num: 0,
            initial_server_sync: false,
            remote_encryption,
        };
        me.read_metadata().await?;
        Ok(me)
    }

    pub fn set_push_batch_size(&mut self, push_batch_size: u32) {
        self.push_batch_size = push_batch_size;
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn pull_frames(
        &mut self,
        generation: u32,
        frame_no: u32,
    ) -> Result<PullResult> {
        let uri = format!(
            "{}/sync/{}/{}/{}",
            self.sync_url,
            generation,
            frame_no,
            // the server expects the range of [start, end) frames, i.e. end is exclusive
            frame_no + self.pull_batch_size
        );
        tracing::debug!("pulling frame (uri={})", uri);
        self.pull_with_retry(uri, self.max_retries).await
    }

    #[tracing::instrument(skip(self, frames))]
    pub(crate) async fn push_frames(
        &mut self,
        frames: Bytes,
        generation: u32,
        frame_no: u32,
        frames_count: u32,
        baton: Option<String>,
    ) -> Result<PushFramesResult> {
        let uri = {
            let mut uri = format!(
                "{}/sync/{}/{}/{}",
                self.sync_url,
                generation,
                frame_no,
                frame_no + frames_count
            );
            if let Some(ref baton) = baton {
                uri.push_str(&format!("/{}", baton));
            }
            uri
        };

        tracing::debug!(
            "pushing frame(frame_no={} (to={}), count={}, generation={}, baton={:?})",
            frame_no,
            frame_no + frames_count,
            frames_count,
            generation,
            baton
        );

        let result = self.push_with_retry(uri, frames, self.max_retries).await?;

        match result.status {
            PushStatus::Conflict => {
                return Err(
                    SyncError::InvalidPushFrameConflict(frame_no, result.max_frame_no).into(),
                );
            }
            _ => {}
        }
        let generation = result.generation;
        let durable_frame_num = result.max_frame_no;
        let baton = result.baton;

        if durable_frame_num > frame_no + frames_count - 1 {
            tracing::error!(
                "server returned durable_frame_num larger than what we sent: sent={}, got={}",
                frame_no,
                durable_frame_num
            );

            return Err(SyncError::InvalidPushFrameNoHigh(frame_no, durable_frame_num).into());
        }

        if durable_frame_num < frame_no + frames_count - 1 {
            // Update our knowledge of where the server is at frame wise.
            self.durable_frame_num = durable_frame_num;

            tracing::debug!(
                "server returned durable_frame_num lower than what we sent: sent={}, got={}",
                frame_no,
                durable_frame_num
            );

            // Return an error and expect the caller to re-call push with the updated state.
            return Err(SyncError::InvalidPushFrameNoLow(frame_no, durable_frame_num).into());
        }

        tracing::debug!(?durable_frame_num, "frame successfully pushed");

        // Update our last known max_frame_no from the server.
        tracing::debug!(
            ?generation,
            ?durable_frame_num,
            "updating remote generation and durable_frame_num"
        );
        self.durable_generation = generation;
        self.durable_frame_num = durable_frame_num;

        Ok(PushFramesResult {
            max_frame_no: durable_frame_num,
            baton,
        })
    }

    async fn push_with_retry(
        &self,
        mut uri: String,
        body: Bytes,
        max_retries: usize,
    ) -> Result<PushResult> {
        let mut nr_retries = 0;
        loop {
            let mut req = http::Request::post(uri.clone());

            match &self.auth_token {
                Some(auth_token) => {
                    req.headers_mut()
                        .expect("valid http request")
                        .insert("Authorization", auth_token.clone());
                }
                None => {}
            }

            if let Some(remote_encryption) = &self.remote_encryption {
                req = req.header("x-turso-encryption-key", remote_encryption.key.as_string());
            }

            let req = req.body(body.clone().into()).expect("valid body");

            let res = self
                .client
                .request(req)
                .await
                .map_err(SyncError::HttpDispatch)?;

            if res.status().is_success() {
                let res_body = hyper::body::to_bytes(res.into_body())
                    .await
                    .map_err(SyncError::HttpBody)?;

                let resp = serde_json::from_slice::<serde_json::Value>(&res_body[..])
                    .map_err(SyncError::JsonDecode)?;

                let status = resp
                    .get("status")
                    .ok_or_else(|| SyncError::JsonValue(resp.clone()))?;

                let status = status
                    .as_str()
                    .ok_or_else(|| SyncError::JsonValue(status.clone()))?;

                let generation = resp
                    .get("generation")
                    .ok_or_else(|| SyncError::JsonValue(resp.clone()))?;

                let generation = generation
                    .as_u64()
                    .ok_or_else(|| SyncError::JsonValue(generation.clone()))?;

                let max_frame_no = resp
                    .get("max_frame_no")
                    .ok_or_else(|| SyncError::JsonValue(resp.clone()))?;

                let max_frame_no = max_frame_no
                    .as_u64()
                    .ok_or_else(|| SyncError::JsonValue(max_frame_no.clone()))?;

                let baton = resp
                    .get("baton")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                tracing::trace!(
                    ?baton,
                    ?generation,
                    ?max_frame_no,
                    ?status,
                    "pushed frame to server"
                );

                let status = match status {
                    "ok" => PushStatus::Ok,
                    "conflict" => PushStatus::Conflict,
                    _ => return Err(SyncError::JsonValue(resp.clone()).into()),
                };
                let generation = generation as u32;
                let max_frame_no = max_frame_no as u32;
                return Ok(PushResult {
                    status,
                    generation,
                    max_frame_no,
                    baton,
                });
            }

            if res.status().is_redirection() {
                uri = match res.headers().get(hyper::header::LOCATION) {
                    Some(loc) => loc.to_str().map_err(SyncError::RedirectHeader)?.to_string(),
                    None => return Err(SyncError::NoRedirectLocationHeader.into()),
                };
                if nr_retries == 0 {
                    nr_retries += 1;
                    continue;
                }
            }

            // If we've retried too many times or the error is not a server error,
            // return the error.
            if nr_retries > max_retries || !res.status().is_server_error() {
                let status = res.status();

                let res_body = hyper::body::to_bytes(res.into_body())
                    .await
                    .map_err(SyncError::HttpBody)?;

                let msg = String::from_utf8_lossy(&res_body[..]);

                return Err(SyncError::PushFrame(status, msg.to_string()).into());
            }

            let delay = std::time::Duration::from_millis(100 * (1 << nr_retries));
            tokio::time::sleep(delay).await;
            nr_retries += 1;
        }
    }

    async fn pull_with_retry(&self, mut uri: String, max_retries: usize) -> Result<PullResult> {
        let mut nr_retries = 0;
        loop {
            let mut req = http::Request::builder().method("GET").uri(uri.clone());

            match &self.auth_token {
                Some(auth_token) => {
                    req = req.header("Authorization", auth_token);
                }
                None => {}
            }

            if let Some(remote_encryption) = &self.remote_encryption {
                req = req.header("x-turso-encryption-key", remote_encryption.key.as_string());
            }

            let req = req.body(Body::empty()).expect("valid request");

            let res = self
                .client
                .request(req)
                .await
                .map_err(SyncError::HttpDispatch)?;

            if res.status().is_success() {
                let frames = hyper::body::to_bytes(res.into_body())
                    .await
                    .map_err(SyncError::HttpBody)?;
                // a success result should always return some frames
                if frames.is_empty() {
                    tracing::error!("server returned empty frames in pull response");
                    return Err(SyncError::InvalidPullFrameBytes(0).into());
                }
                // the minimum payload size cannot be less than a single frame
                if frames.len() < FRAME_SIZE {
                    tracing::error!(
                        "server returned frames with invalid length: {} < {}",
                        frames.len(),
                        FRAME_SIZE
                    );
                    return Err(SyncError::InvalidPullFrameBytes(frames.len()).into());
                }
                return Ok(PullResult::Frames(frames));
            }
            // BUG ALERT: The server returns a 500 error if the remote database is empty.
            // This is a bug and should be fixed.
            if res.status() == StatusCode::BAD_REQUEST
                || res.status() == StatusCode::INTERNAL_SERVER_ERROR
            {
                let status = res.status();
                let res_body = hyper::body::to_bytes(res.into_body())
                    .await
                    .map_err(SyncError::HttpBody)?;
                tracing::trace!(
                    "server returned: {} body: {}",
                    status,
                    String::from_utf8_lossy(&res_body[..])
                );
                let resp = serde_json::from_slice::<serde_json::Value>(&res_body[..])
                    .map_err(SyncError::JsonDecode)?;

                let generation = resp
                    .get("generation")
                    .ok_or_else(|| SyncError::JsonValue(resp.clone()))?;

                let generation = generation
                    .as_u64()
                    .ok_or_else(|| SyncError::JsonValue(generation.clone()))?;
                return Ok(PullResult::EndOfGeneration {
                    max_generation: generation as u32,
                });
            }
            if res.status().is_redirection() {
                uri = match res.headers().get(hyper::header::LOCATION) {
                    Some(loc) => loc.to_str().map_err(SyncError::RedirectHeader)?.to_string(),
                    None => return Err(SyncError::NoRedirectLocationHeader.into()),
                };
                if nr_retries == 0 {
                    nr_retries += 1;
                    continue;
                }
            }
            // If we've retried too many times or the error is not a server error,
            // return the error.
            if nr_retries > max_retries || !res.status().is_server_error() {
                let status = res.status();

                let res_body = hyper::body::to_bytes(res.into_body())
                    .await
                    .map_err(SyncError::HttpBody)?;

                let msg = String::from_utf8_lossy(&res_body[..]);

                return Err(SyncError::PullFrame(status, msg.to_string()).into());
            }

            let delay = std::time::Duration::from_millis(100 * (1 << nr_retries));
            tokio::time::sleep(delay).await;
            nr_retries += 1;
        }
    }

    pub(crate) fn next_generation(&mut self) {
        self.durable_generation += 1;
        self.durable_frame_num = 0;
    }

    pub(crate) fn durable_frame_num(&self) -> u32 {
        self.durable_frame_num
    }

    pub(crate) fn durable_generation(&self) -> u32 {
        self.durable_generation
    }

    pub(crate) async fn write_metadata(&mut self) -> Result<()> {
        let path = format!("{}-info", self.db_path);

        assert!(self.durable_generation > 0);

        let mut metadata = MetadataJson {
            hash: 0,
            version: METADATA_VERSION,
            durable_frame_num: self.durable_frame_num,
            generation: self.durable_generation,
        };

        metadata.set_hash();

        let contents = serde_json::to_vec(&metadata).map_err(SyncError::JsonEncode)?;

        atomic_write(path, &contents[..]).await?;

        Ok(())
    }

    async fn read_metadata(&mut self) -> Result<()> {
        let path = format!("{}-info", self.db_path);

        if !Path::new(&path)
            .try_exists()
            .map_err(SyncError::io("metadata file exists"))?
        {
            tracing::debug!("no metadata info file found");
            return Ok(());
        }

        let contents = tokio::fs::read(&path)
            .await
            .map_err(SyncError::io("metadata read"))?;

        let metadata =
            serde_json::from_slice::<MetadataJson>(&contents[..]).map_err(SyncError::JsonDecode)?;

        metadata.verify_hash()?;

        if metadata.version != METADATA_VERSION {
            return Err(SyncError::VerifyVersion(metadata.version, METADATA_VERSION).into());
        }

        tracing::debug!(
            "read sync metadata for db_path={:?}, metadata={:?}",
            self.db_path,
            metadata
        );

        if metadata.generation == 0 {
            return Err(SyncError::InvalidLocalState("generation is 0".to_string()).into());
        }

        self.durable_generation = metadata.generation;
        self.durable_frame_num = metadata.durable_frame_num;

        Ok(())
    }

    /// get_remote_info calls the remote server to get the current generation information.
    async fn get_remote_info(&self) -> Result<InfoResult> {
        let uri = format!("{}/info", self.sync_url);
        let mut req = http::Request::builder().method("GET").uri(&uri);

        if let Some(auth_token) = &self.auth_token {
            req = req.header("Authorization", auth_token);
        }

        if let Some(remote_encryption) = &self.remote_encryption {
            req = req.header("x-turso-encryption-key", remote_encryption.key.as_string());
        }

        let req = req.body(Body::empty()).expect("valid request");

        let res = self
            .client
            .request(req)
            .await
            .map_err(SyncError::HttpDispatch)?;

        if !res.status().is_success() {
            let status = res.status();
            let body = hyper::body::to_bytes(res.into_body())
                .await
                .map_err(SyncError::HttpBody)?;
            return Err(
                SyncError::PullDb(status, String::from_utf8_lossy(&body).to_string()).into(),
            );
        }

        let body = hyper::body::to_bytes(res.into_body())
            .await
            .map_err(SyncError::HttpBody)?;

        let info: InfoResult = serde_json::from_slice(&body).map_err(SyncError::JsonDecode)?;
        if info.current_generation == 0 {
            return Err(SyncError::InvalidRemoteState("generation is 0".to_string()).into());
        }
        Ok(info)
    }

    async fn sync_db_if_needed(&mut self) -> Result<()> {
        let db_file_exists = check_if_file_exists(&self.db_path)?;
        let metadata_exists = check_if_file_exists(&format!("{}-info", self.db_path))?;
        if db_file_exists && metadata_exists {
            return Ok(());
        }
        let info = self.get_remote_info().await?;
        let generation = info.current_generation;
        // somehow we are ahead of the remote in generations. following should not happen because
        // we checkpoint only if the remote server tells us to do so.
        if self.durable_generation > generation {
            tracing::error!(
                "server returned a lower generation than what we have: local={}, remote={}",
                self.durable_generation,
                generation
            );
            return Err(
                SyncError::InvalidLocalGeneration(self.durable_generation, generation).into(),
            );
        }
        // we use the following heuristic to determine if we need to sync the db file
        // 1. if no db file or the metadata file exists, then user is starting from scratch
        //    and we will do the sync
        // 2. if the db file exists, but the metadata file does not exist (or other way around),
        //    then local db is in an incorrect state. we stop and return with an error
        // 3. if the db file exists and the metadata file exists, then we don't need to do the
        //    sync
        match (metadata_exists, db_file_exists) {
            (false, false) => {
                // neither the db file nor the metadata file exists, lets bootstrap from remote
                tracing::debug!(
                    "syncing db file from remote server, generation={}",
                    generation
                );
                self.sync_db(generation).await
            }
            (false, true) => {
                // inconsistent state: DB exists but metadata missing
                tracing::error!(
                    "local state is incorrect, db file exists but metadata file does not"
                );
                Err(SyncError::InvalidLocalState(
                    "db file exists but metadata file does not".to_string(),
                )
                .into())
            }
            (true, false) => {
                // inconsistent state: Metadata exists but DB missing
                tracing::error!(
                    "local state is incorrect, metadata file exists but db file does not"
                );
                Err(SyncError::InvalidLocalState(
                    "metadata file exists but db file does not".to_string(),
                )
                .into())
            }
            (true, true) => {
                // We already handled this case earlier in the function.
                unreachable!();
            }
        }
    }

    /// sync_db will download the db file from the remote server and replace the local file.
    async fn sync_db(&mut self, generation: u32) -> Result<()> {
        let uri = format!("{}/export/{}", self.sync_url, generation);
        let mut req = http::Request::builder().method("GET").uri(&uri);

        if let Some(auth_token) = &self.auth_token {
            req = req.header("Authorization", auth_token);
        }

        if let Some(remote_encryption) = &self.remote_encryption {
            req = req.header("x-turso-encryption-key", remote_encryption.key.as_string());
        }

        let req = req.body(Body::empty()).expect("valid request");

        let (res, http_duration) =
            crate::replication::remote_client::time(self.client.request(req)).await;
        let res = res.map_err(SyncError::HttpDispatch)?;

        if !res.status().is_success() {
            let status = res.status();
            let body = hyper::body::to_bytes(res.into_body())
                .await
                .map_err(SyncError::HttpBody)?;
            tracing::error!(
                "failed to pull db file from remote server, status={}, body={}, url={}, duration={:?}",
                status,
                String::from_utf8_lossy(&body),
                uri,
                http_duration
            );
            return Err(
                SyncError::PullFrame(status, String::from_utf8_lossy(&body).to_string()).into(),
            );
        }

        tracing::debug!(
            "pulled db file from remote server, status={}, url={}, duration={:?}",
            res.status(),
            uri,
            http_duration
        );

        // todo: do streaming write to the disk
        let bytes = hyper::body::to_bytes(res.into_body())
            .await
            .map_err(SyncError::HttpBody)?;

        atomic_write(&self.db_path, &bytes).await?;
        self.durable_generation = generation;
        self.durable_frame_num = 0;
        self.write_metadata().await?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct MetadataJson {
    hash: u32,
    version: u32,
    durable_frame_num: u32,
    generation: u32,
}

impl MetadataJson {
    fn calculate_hash(&self) -> u32 {
        let mut hasher = crc32fast::Hasher::new();

        // Hash each field in a consistent order
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&self.durable_frame_num.to_le_bytes());
        hasher.update(&self.generation.to_le_bytes());

        hasher.finalize()
    }

    fn set_hash(&mut self) {
        self.hash = self.calculate_hash();
    }

    fn verify_hash(&self) -> Result<()> {
        let calculated_hash = self.calculate_hash();

        if self.hash == calculated_hash {
            Ok(())
        } else {
            Err(SyncError::VerifyHash(self.hash, calculated_hash).into())
        }
    }
}

async fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<()> {
    // Create a temporary file in the same directory as the target file
    let directory = path.as_ref().parent().ok_or_else(|| {
        SyncError::io("parent path")(std::io::Error::other(
            "unable to get parent of the provided path",
        ))
    })?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let temp_name = format!(".tmp.{}.{}", timestamp, Uuid::new_v4());
    let temp_path = directory.join(temp_name);

    // Write data to temporary file
    let mut temp_file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(SyncError::io("temp file create"))?;

    temp_file
        .write_all(data)
        .await
        .map_err(SyncError::io("temp file write_all"))?;

    // Ensure all data is flushed to disk
    temp_file
        .sync_all()
        .await
        .map_err(SyncError::io("temp file sync_all"))?;

    // Close the file explicitly
    drop(temp_file);

    // Atomically rename temporary file to target file
    tokio::fs::rename(&temp_path, &path)
        .await
        .map_err(SyncError::io("atomic rename"))?;

    Ok(())
}

/// bootstrap_db brings the .db file from remote, if required. If the .db file already exists, then
/// it does nothing. Calling this function multiple times is safe.
/// However, make sure there are no existing active connections to the db file as this method can
/// replace it
pub async fn bootstrap_db(sync_ctx: &mut SyncContext) -> Result<()> {
    // todo: we are checking with the remote server only during initialisation. ideally,
    // we need to do this when we notice a large gap in generations, when bootstrapping is cheaper
    // than pulling each frame
    if !sync_ctx.initial_server_sync {
        sync_ctx.sync_db_if_needed().await?;
        // when sync_ctx is initialised, we set durable_generation to 0. however, once
        // sync_db is called, it should be > 0.
        assert!(sync_ctx.durable_generation > 0, "generation should be > 0");
        sync_ctx.initial_server_sync = true;
    }
    Ok(())
}

/// Sync WAL frames to remote.
pub async fn sync_offline(
    sync_ctx: &mut SyncContext,
    conn: &Connection,
) -> Result<crate::database::Replicated> {
    if is_ahead_of_remote(&sync_ctx, &conn) {
        match try_push(sync_ctx, conn).await {
            Ok(rep) => Ok(rep),
            Err(Error::Sync(err)) => {
                // Retry the sync because we are ahead of the server and we need to push some older
                // frames.
                if let Some(SyncError::InvalidPushFrameNoLow(_, _)) = err.downcast_ref() {
                    tracing::debug!("got InvalidPushFrameNo, retrying push");
                    try_push(sync_ctx, conn).await
                } else {
                    Err(Error::Sync(err))
                }
            }
            Err(e) => Err(e),
        }
    } else {
        try_pull(sync_ctx, conn).await
    }
    .or_else(|err| {
        let Error::Sync(err) = err else {
            return Err(err);
        };

        // TODO(levy): upcasting should be done *only* at the API boundary, doing this in
        // internal code just sucks.
        let Some(SyncError::HttpDispatch(_)) = err.downcast_ref() else {
            return Err(Error::Sync(err));
        };

        Ok(crate::database::Replicated {
            frame_no: None,
            frames_synced: 0,
        })
    })
}

fn is_ahead_of_remote(sync_ctx: &SyncContext, conn: &Connection) -> bool {
    let max_local_frame = conn.wal_frame_count();
    max_local_frame > sync_ctx.durable_frame_num()
}

async fn try_push(
    sync_ctx: &mut SyncContext,
    conn: &Connection,
) -> Result<crate::database::Replicated> {
    let page_size = {
        let rows = conn
            .query("PRAGMA page_size", crate::params::Params::None)?
            .unwrap();
        let row = rows.next()?.unwrap();
        let page_size = row.get::<u32>(0)?;
        page_size
    };

    let max_frame_no = conn.wal_frame_count();
    if max_frame_no == 0 {
        return Ok(crate::database::Replicated {
            frame_no: None,
            frames_synced: 0,
        });
    }

    let generation = sync_ctx.durable_generation();
    let start_frame_no = sync_ctx.durable_frame_num() + 1;
    let end_frame_no = max_frame_no;
    let mut baton = None;

    let mut frame_no = start_frame_no;
    while frame_no <= end_frame_no {
        let batch_size = sync_ctx.push_batch_size.min(end_frame_no - frame_no + 1);
        let mut frames = conn.wal_get_frame(frame_no, page_size)?;
        if batch_size > 1 {
            frames.reserve((batch_size - 1) as usize * frames.len());
        }
        for idx in 1..batch_size {
            let frame = conn.wal_get_frame(frame_no + idx, page_size)?;
            frames.extend_from_slice(frame.as_ref())
        }

        // The server returns its maximum frame number. To avoid resending
        // frames the server already knows about, we need to update the
        // frame number to the one returned by the server.
        let result = sync_ctx
            .push_frames(frames.freeze(), generation, frame_no, batch_size, baton)
            .await?;
        // if the server sent us a baton, then we will reuse it for the next request
        baton = result.baton;
        let max_frame_no = result.max_frame_no;

        if max_frame_no > frame_no {
            frame_no = max_frame_no + 1;
        } else {
            frame_no += batch_size;
        }
    }

    sync_ctx.write_metadata().await?;

    // TODO(lucio): this can underflow if the server previously returned a higher max_frame_no
    // than what we have stored here.
    let frame_count = end_frame_no - start_frame_no + 1;
    Ok(crate::database::Replicated {
        frame_no: None,
        frames_synced: frame_count as usize,
    })
}

/// PAGE_SIZE used by the sync / diskless server
const PAGE_SIZE: usize = 4096;
const FRAME_HEADER_SIZE: usize = 24;
const FRAME_SIZE: usize = PAGE_SIZE + FRAME_HEADER_SIZE;

pub async fn try_pull(
    sync_ctx: &mut SyncContext,
    conn: &Connection,
) -> Result<crate::database::Replicated> {
    // note, that updates of durable_frame_num are valid only after SQLite commited the WAL
    // (because if WAL has uncommited suffix - it will be omitted by any other SQLite connection - for example after restart)
    // so, try_pull maintains local next_frame_no during the pull operation and update durable_frame_num when it's appropriate
    let mut next_frame_no = sync_ctx.durable_frame_num + 1;

    // libsql maintain consistent state about WAL sync session locally in the insert_handle
    // note, that insert_handle will always close the session on drop - so we never keep active WAL session after we exit from the method
    let insert_handle = conn.wal_insert_handle();

    loop {
        // get current generation (it may be updated multiple times during execution)
        let generation = sync_ctx.durable_generation();

        match sync_ctx.pull_frames(generation, next_frame_no).await {
            Ok(PullResult::Frames(frames)) => {
                tracing::debug!(
                    "pull_frames: generation={}, start_frame={} (end_frame={}, batch_size={}), frames_size={}",
                    generation, next_frame_no, next_frame_no + sync_ctx.pull_batch_size, sync_ctx.pull_batch_size, frames.len(),
                );
                if frames.len() % FRAME_SIZE != 0 {
                    tracing::error!(
                        "frame size {} is not a multiple of the expected size {}",
                        frames.len(),
                        FRAME_SIZE,
                    );
                    return Err(SyncError::InvalidPullFrameBytes(frames.len()).into());
                }
                for chunk in frames.chunks(FRAME_SIZE) {
                    let mut size_after_buf = [0u8; 4];
                    size_after_buf.copy_from_slice(&chunk[4..8]);
                    let size_after = big_endian::U32::from_bytes(size_after_buf);
                    // start WAL sync session if it was closed
                    // (this can happen if on previous iteration client received commit frame)
                    if !insert_handle.in_session() {
                        tracing::debug!(
                            "pull_frames: generation={}, frame={}, start wal transaction session",
                            generation,
                            next_frame_no
                        );
                        insert_handle.begin()?;
                    }
                    let result = insert_handle.insert_at(next_frame_no, &chunk);
                    if let Err(e) = result {
                        tracing::error!("insert error (frame={}) : {:?}", next_frame_no, e);
                        return Err(e);
                    }
                    // if this is commit frame - we can close WAL sync session and update durable_frame_num
                    if size_after.get() > 0 {
                        tracing::debug!(
                            "pull_frames: generation={}, frame={}, finish wal transaction session, size_after={}",
                            generation,
                            next_frame_no,
                            size_after.get()
                        );
                        insert_handle.end()?;
                        sync_ctx.durable_frame_num = next_frame_no;
                        sync_ctx.write_metadata().await?;
                    }

                    next_frame_no += 1;
                }
            }
            Ok(PullResult::EndOfGeneration { max_generation }) => {
                // If there are no more generations to pull, we're done.
                if generation >= max_generation {
                    break;
                }
                assert!(
                    !insert_handle.in_session(),
                    "WAL transaction must be finished"
                );

                tracing::debug!(
                    "pull_frames: generation={}, frame={}, checkpoint in order to move to next generation",
                    generation,
                    next_frame_no
                );
                // TODO: Make this crash-proof.
                conn.wal_checkpoint(true)?;

                sync_ctx.next_generation();
                sync_ctx.write_metadata().await?;
                next_frame_no = 1;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(crate::database::Replicated {
        frame_no: None,
        frames_synced: 1,
    })
}

fn check_if_file_exists(path: &str) -> core::result::Result<bool, SyncError> {
    Path::new(&path)
        .try_exists()
        .map_err(SyncError::io("metadata file exists"))
}

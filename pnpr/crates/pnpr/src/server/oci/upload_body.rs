use super::{
    BlobUpload, Body, Bytes, CanonicalPackageName, Digest, ErrorCode, HASH_CHUNK, Refusal,
    RegistryError, Sha256, Storage, SystemTime, UNIX_EPOCH,
};
use futures_util::StreamExt as _;
use sha2::Digest as _;
use tokio::io::AsyncReadExt;

/// The inclusive bounds a `Content-Range: <start>-<end>` names.
///
/// Both are parsed and kept: reading only the text before the hyphen would
/// accept `0-garbage` whenever its leading number happened to match the
/// offset, and discarding the end would accept `5-2`. Either lets a client
/// advance an upload under a range that means nothing.
pub(super) fn parse_content_range(range: &str) -> Option<(u64, u64)> {
    let (start, end) = range.trim().split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    (start <= end).then_some((start, end))
}

pub(super) async fn collect_body(body: Body, limit: usize) -> Result<Bytes, Refusal> {
    axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| Refusal::new(ErrorCode::SizeInvalid, "request body is too large or truncated"))
}

/// Stream a request body onto the end of an upload, holding the whole upload
/// to the configured byte limit. The per-request body limit cannot do that on its
/// own: a resumable upload is many requests, each one under the limit.
///
/// An upload that runs over is dropped rather than kept truncated at the
/// ceiling, because what was sent is not a blob anyone asked for.
///
/// A stream that ends early is kept instead. The bytes written are an ordered
/// prefix of the blob, which is the state a resumable upload exists to hold:
/// discarding it would cost a client the whole of a multi-gigabyte layer for
/// one dropped connection. The refusal a later chunk gets carries where the
/// upload actually stands, so the client resumes from the prefix, and the
/// digest check at `PUT` is what decides whether the assembled bytes are the
/// blob that was promised.
pub(super) async fn append_body(
    storage: &Storage,
    upload: &BlobUpload,
    body: Body,
    limit: u64,
) -> Result<(), Refusal> {
    let mut written = upload.offset().await?;
    if written > limit {
        storage.abort_blob_upload(upload.id()).await?;
        return Err(Refusal::new(
            ErrorCode::SizeInvalid,
            format!("a blob may not exceed {limit} bytes"),
        ));
    }
    let mut writer = upload.append().await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            writer.finish().await?;
            return Err(Refusal::new(ErrorCode::BlobUploadInvalid, "upload stream ended early"));
        };
        let Some(next) = advance_within_ceiling(written, chunk.len(), limit) else {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return Err(Refusal::new(
                ErrorCode::SizeInvalid,
                format!("a blob may not exceed {limit} bytes"),
            ));
        };
        written = next;
        writer.write_all(&chunk).await?;
    }
    writer.finish().await?;
    Ok(())
}

/// The upload's length once `chunk` is accepted, or `None` when that would
/// take it past the configured byte limit. Overflow refuses the chunk.
pub(super) fn advance_within_ceiling(written: u64, chunk: usize, limit: u64) -> Option<u64> {
    let next = written.checked_add(chunk as u64)?;
    (next <= limit).then_some(next)
}

/// Milliseconds since the Unix epoch: the ordering one tag write carries.
pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX))
}

/// An upload's key in the shared lock table, kept out of the package keyspace
/// so an upload and a publish of the same name never contend by accident.
pub(super) fn upload_lock_key(id: &str) -> String {
    format!("oci-upload:{id}")
}

/// Hash a finished upload without holding it in memory.
pub(super) async fn hash_upload(upload: &BlobUpload) -> Result<Digest, RegistryError> {
    upload.materialize().await?;
    let mut file = tokio::fs::File::open(upload.path()).await.map_err(RegistryError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        let read = file.read(&mut buffer).await.map_err(RegistryError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Digest::parse(&format!("sha256:{:x}", hasher.finalize()))
        .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })
}

pub(super) async fn read_manifest_bytes(
    storage: &Storage,
    key: &CanonicalPackageName,
    filename: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let Some((body, _)) = storage.open_hosted_blob(key, filename).await? else {
        return Ok(None);
    };
    let bytes = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| RegistryError::BadRequest { reason: "manifest is too large".to_string() })?;
    Ok(Some(bytes.to_vec()))
}

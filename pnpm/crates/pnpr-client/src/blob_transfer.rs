//! Moving one blob between a file and a server without holding it whole.

use super::{ArtifactPayload, BTreeMap, HashSet, PnprClientError, SignedArtifactEnvelope};
use futures_util::StreamExt as _;
use pnpm_shared_artifact_protocol::verify_blob_digest;
use sha2::{Digest as _, Sha512};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt as _;

/// One blob a publication uploads, read from the file at `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBlobSource {
    pub integrity: String,
    pub size: u64,
    pub path: PathBuf,
}

/// A signed artifact, and the files its manifest's blobs are read from.
#[derive(Debug, Clone)]
pub struct ArtifactPublication {
    pub key: String,
    pub envelope: SignedArtifactEnvelope,
    pub blobs: Vec<ArtifactBlobSource>,
}

impl ArtifactPublication {
    /// The signed payload, once the key is the one it signs and every blob
    /// its manifest adds has exactly one source of the declared size.
    pub fn validate(&self) -> Result<ArtifactPayload, PnprClientError> {
        let protocol = |reason: String| PnprClientError::Protocol(reason);
        let (payload, _) =
            self.envelope.decode_payload().map_err(|err| protocol(err.to_string()))?;
        if payload.input_key != self.key {
            return Err(
                protocol("signed input key does not match the publication key".to_string()),
            );
        }
        let required: BTreeMap<&str, u64> = payload.manifest.added
            .iter()
            .map(|file| (file.integrity.as_str(), file.size))
            .collect();
        let mut sourced = HashSet::with_capacity(self.blobs.len());
        for blob in &self.blobs {
            if !sourced.insert(blob.integrity.as_str()) {
                return Err(protocol(format!("blob {:?} has two sources", blob.integrity)));
            }
            if required.get(blob.integrity.as_str()) != Some(&blob.size) {
                return Err(protocol(format!(
                    "blob {:?} is not in the signed manifest at {} bytes",
                    blob.integrity, blob.size,
                )));
            }
        }
        if sourced.len() != required.len() {
            return Err(protocol("the signed manifest names a blob with no source".to_string()));
        }
        Ok(payload)
    }
}

/// The slowest transfer a blob request allows for before it times out.
const MIN_TRANSFER_RATE: u64 = 1024 * 1024;

/// How long a request moving a blob of `size` bytes may take: `base`, plus
/// the time the blob takes at [`MIN_TRANSFER_RATE`].
pub(crate) fn blob_timeout(base: Duration, size: u64) -> Duration {
    base + Duration::from_secs(size / MIN_TRANSFER_RATE)
}

/// What an upload request carries: a body streamed from the file on native
/// targets, the file's bytes on WebAssembly, whose HTTP client takes no
/// streamed body.
#[cfg(not(target_family = "wasm"))]
pub(crate) type BlobBody = reqwest::Body;
#[cfg(target_family = "wasm")]
pub(crate) type BlobBody = Vec<u8>;

/// The upload body of `source`, whose file must still hold `source.size`
/// bytes.
pub(crate) async fn blob_body(source: &ArtifactBlobSource) -> Result<BlobBody, PnprClientError> {
    let file = tokio::fs::File::open(&source.path).await?;
    let size = file.metadata().await?.len();
    if size != source.size {
        return Err(PnprClientError::Protocol(format!(
            "{} has {size} bytes, not the {} its artifact declares",
            source.path.display(),
            source.size,
        )));
    }
    file_body(file).await
}

#[cfg(not(target_family = "wasm"))]
async fn file_body(file: tokio::fs::File) -> Result<BlobBody, PnprClientError> {
    Ok(reqwest::Body::wrap_stream(file_chunks(file)))
}

#[cfg(target_family = "wasm")]
async fn file_body(mut file: tokio::fs::File) -> Result<BlobBody, PnprClientError> {
    let mut bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut file, &mut bytes).await?;
    Ok(bytes)
}

/// The file's bytes as a stream of chunks.
#[cfg(not(target_family = "wasm"))]
fn file_chunks(
    file: tokio::fs::File,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + Sync + 'static {
    const CHUNK: usize = 256 * 1024;
    futures_util::stream::try_unfold(file, |mut file| async move {
        let mut chunk = vec![0; CHUNK];
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        chunk.truncate(read);
        Ok(Some((chunk, file)))
    })
}

/// A blob's bytes, held to its declared size and checked against its
/// integrity. Content that does not match is a [`PnprClientError::Protocol`]
/// error.
pub(crate) async fn read_blob(
    response: reqwest::Response,
    integrity: &str,
    size: u64,
) -> Result<Vec<u8>, PnprClientError> {
    let mut check = BlobCheck::new(integrity, size, response.content_length())?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        check.accept(&chunk)?;
        bytes.extend_from_slice(&chunk);
    }
    check.finish()?;
    Ok(bytes)
}

/// [`read_blob`] into a new file at `destination`, which is removed again
/// when the blob is refused.
pub(crate) async fn write_blob(
    response: reqwest::Response,
    integrity: &str,
    size: u64,
    destination: &Path,
) -> Result<(), PnprClientError> {
    let mut check = BlobCheck::new(integrity, size, response.content_length())?;
    let mut file = tokio::fs::File::create_new(destination).await?;
    let written = async {
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            check.accept(&chunk)?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        check.finish()
    }
    .await;
    if written.is_err() {
        drop(file);
        if let Err(error) = tokio::fs::remove_file(destination).await {
            tracing::debug!(target: "pacquet::artifacts", %error, "a refused blob was not removed");
        }
    }
    written
}

/// The bytes of one blob seen so far, against what its manifest declares.
struct BlobCheck<'a> {
    integrity: &'a str,
    size: u64,
    received: u64,
    hasher: Sha512,
}

impl<'a> BlobCheck<'a> {
    fn new(
        integrity: &'a str,
        size: u64,
        content_length: Option<u64>,
    ) -> Result<Self, PnprClientError> {
        if let Some(length) = content_length
            && length != size
        {
            return Err(PnprClientError::Protocol(format!(
                "the server sent {length} bytes for a blob of {size}",
            )));
        }
        Ok(BlobCheck { integrity, size, received: 0, hasher: Sha512::new() })
    }

    fn accept(&mut self, chunk: &[u8]) -> Result<(), PnprClientError> {
        self.received = self.received.saturating_add(chunk.len() as u64);
        if self.received > self.size {
            return Err(PnprClientError::Protocol(format!(
                "the server sent more than the blob's {} bytes",
                self.size,
            )));
        }
        self.hasher.update(chunk);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), PnprClientError> {
        if self.received != self.size {
            return Err(PnprClientError::Protocol(format!(
                "the server sent {} bytes for a blob of {}",
                self.received, self.size,
            )));
        }
        verify_blob_digest(self.integrity, &std::mem::take(&mut self.hasher).finalize())
            .map_err(|err| PnprClientError::Protocol(err.to_string()))
    }
}

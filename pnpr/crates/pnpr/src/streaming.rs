//! Streaming helpers for the tarball path.
//!
//! Four flows live here:
//!
//! * [`verify_file`] hashes a cache hit before it can be served.
//! * [`download_verified_to_cache`] hashes an upstream response into a
//!   temp file and promotes it only after the declared SRI matches.
//! * [`download_verified_to_temp`] hashes an upstream response into a
//!   temp file for mirror-less pass-through.
//! * [`stream_file`] yields an already verified file to the response.

use crate::storage::TarballWrite;
use axum::body::{Body, Bytes};
use futures_util::{StreamExt, stream};
use ssri::{Integrity, IntegrityChecker};
use std::{
    io::{self, SeekFrom},
    path::PathBuf,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
};

/// Chunk size for reading from a cached file. 64 KiB keeps syscall
/// overhead low without buffering a meaningful fraction of a
/// multi-MB tarball.
const READ_CHUNK: usize = 64 * 1024;

pub fn parse_integrity(value: &str) -> Result<Integrity, ssri::Error> {
    let integrity: Integrity = value.parse()?;
    ensure_supported_hash(&integrity)?;
    Ok(integrity)
}

fn ensure_supported_hash(integrity: &Integrity) -> Result<(), ssri::Error> {
    if integrity.hashes.is_empty() {
        return Err(ssri::Error::ParseIntegrityError(
            "integrity string contains no supported hashes".to_string(),
        ));
    }
    Ok(())
}

pub fn integrity_checker(integrity: &Integrity) -> Result<IntegrityChecker, ssri::Error> {
    ensure_supported_hash(integrity)?;
    Ok(IntegrityChecker::new(integrity.clone()))
}

#[derive(Debug)]
pub enum TarballStreamError {
    Upstream { url: String, source: reqwest::Error },
    Io(io::Error),
    Integrity(ssri::Error),
    TooLarge { limit: u64, received: u64 },
}

/// Hash a cached file against `integrity`, then rewind the same open
/// file so the caller can stream the exact bytes that were checked.
pub async fn verify_file(
    mut file: File,
    integrity: &Integrity,
) -> Result<File, TarballStreamError> {
    let mut checker = integrity_checker(integrity).map_err(TarballStreamError::Integrity)?;
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let bytes_read = file.read(&mut buf).await.map_err(TarballStreamError::Io)?;
        if bytes_read == 0 {
            break;
        }
        checker.input(&buf[..bytes_read]);
    }
    checker.result().map_err(TarballStreamError::Integrity)?;
    file.seek(SeekFrom::Start(0)).await.map_err(TarballStreamError::Io)?;
    Ok(file)
}

/// Download an upstream response into `write`, verify the complete body,
/// and atomically promote it to the cache. No bytes become cache-visible
/// until the declared SRI matches.
pub async fn download_verified_to_cache(
    response: reqwest::Response,
    mut write: TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<u64, TarballStreamError> {
    let len = match download_verified(response, &mut write, integrity, max_bytes).await {
        Ok(len) => len,
        Err(err) => {
            write.abandon().await;
            return Err(err);
        }
    };
    write.finalize().await.map_err(TarballStreamError::Io)?;
    Ok(len)
}

pub async fn download_verified_to_temp(
    response: reqwest::Response,
    mut write: TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<(File, u64, PathBuf), TarballStreamError> {
    if let Err(err) = download_verified(response, &mut write, integrity, max_bytes).await {
        write.abandon().await;
        return Err(err);
    }
    write.into_temp_file().await.map_err(TarballStreamError::Io)
}

async fn download_verified(
    response: reqwest::Response,
    write: &mut TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<u64, TarballStreamError> {
    let url = response.url().to_string();
    if let Some(received) = response.content_length()
        && received > max_bytes
    {
        return Err(TarballStreamError::TooLarge { limit: max_bytes, received });
    }
    let mut upstream = response.bytes_stream();
    let mut checker = integrity_checker(integrity).map_err(TarballStreamError::Integrity)?;
    let mut written = 0u64;
    while let Some(chunk_result) = upstream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(source) => return Err(TarballStreamError::Upstream { url, source }),
        };
        let received = written.saturating_add(chunk.len() as u64);
        if received > max_bytes {
            return Err(TarballStreamError::TooLarge { limit: max_bytes, received });
        }
        if let Err(err) = write.write_all(&chunk).await {
            return Err(TarballStreamError::Io(err));
        }
        checker.input(&chunk);
        written = received;
    }

    if let Err(err) = checker.result() {
        return Err(TarballStreamError::Integrity(err));
    }
    Ok(written)
}

/// Stream a cached file as a response body. Caller is responsible for
/// setting `Content-Length` (from the file metadata it already read).
pub fn stream_file(file: File) -> Body {
    // Carry the `File` through the unfold *state* (not as a closure
    // capture) so each step owns it, reads, and hands it back. An
    // `FnMut` closure can't move the file across iterations on its
    // own.
    let stream = stream::unfold(Some(file), |state| async move {
        let mut file = state?;
        let mut buf = vec![0u8; READ_CHUNK];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, io::Error>(Bytes::from(buf)), Some(file)))
            }
            Err(err) => Some((Err(err), None)),
        }
    });
    Body::from_stream(stream)
}

pub fn stream_file_and_remove(file: File, path: PathBuf) -> Body {
    let stream = stream::unfold(Some(RemoveOnDropFile::new(file, path)), |state| async move {
        let mut state = state?;
        let mut buf = vec![0u8; READ_CHUNK];
        let file = state.file.as_mut().expect("file is present until stream finishes");
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, io::Error>(Bytes::from(buf)), Some(state)))
            }
            Err(err) => Some((Err(err), None)),
        }
    });
    Body::from_stream(stream)
}

struct RemoveOnDropFile {
    file: Option<File>,
    path: Option<PathBuf>,
}

impl RemoveOnDropFile {
    fn new(file: File, path: PathBuf) -> Self {
        Self { file: Some(file), path: Some(path) }
    }
}

impl Drop for RemoveOnDropFile {
    fn drop(&mut self) {
        drop(self.file.take());
        let Some(path) = self.path.take() else { return };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(?err, path = %path.display(), "temporary tarball cleanup failed");
            }
        }
    }
}

#[cfg(test)]
mod tests;

use super::CacheError;
use crate::store::remove_tree;
use bytes::{
    Buf as _,
    Bytes,
};
use lzma_rust2::XzReader;
use pnpz_nix_store::{
    Compression,
    NarInfo,
    nar::{
        HashingReader,
        UnpackError,
        unpack,
    },
};
use std::{
    io::{
        self,
        BufReader,
        Read,
    },
    path::Path,
};
use tokio::sync::mpsc::Receiver;

/// Presents the chunks an async download sends as a blocking reader.
struct ChunkReader {
    chunks: Receiver<io::Result<Bytes>>,
    current: Bytes,
}

impl Read for ChunkReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        while self.current.is_empty() {
            match self.chunks.blocking_recv() {
                Some(chunk) => self.current = chunk?,
                None => return Ok(0),
            }
        }
        let len = buffer.len().min(self.current.len());
        self.current.copy_to_slice(&mut buffer[..len]);
        Ok(len)
    }
}

/// Decompresses and unpacks the archive arriving on `chunks`, then checks it
/// against the hash and size `narinfo` signs.
pub(super) fn unpack_verified(
    chunks: Receiver<io::Result<Bytes>>,
    narinfo: &NarInfo,
    destination: &Path,
) -> Result<(), CacheError> {
    let outcome = unpack_and_check(chunks, narinfo, destination);
    if outcome.is_err() {
        // The tree is unverified. Losing the cleanup error is fine: the
        // caller reports why the download failed, and a leftover staging
        // directory is never read.
        let _ = remove_tree(destination);
    }
    outcome
}

fn unpack_and_check(
    chunks: Receiver<io::Result<Bytes>>,
    narinfo: &NarInfo,
    destination: &Path,
) -> Result<(), CacheError> {
    let store_path = narinfo.store_path.to_string();
    let unpack_error = |source| CacheError::Unpack { store_path: store_path.clone(), source };
    let compressed = ChunkReader { chunks, current: Bytes::new() };
    let archive = decompress(narinfo.compression, compressed)
        .map_err(UnpackError::Read)
        .map_err(&unpack_error)?;
    let mut archive = HashingReader::new(BufReader::new(archive));
    unpack(&mut archive, destination).map_err(&unpack_error)?;
    let (hash, size) = archive
        .finish()
        .map_err(UnpackError::Read)
        .map_err(&unpack_error)?;
    if (hash, size) != (narinfo.nar_hash, narinfo.nar_size) {
        return Err(CacheError::HashMismatch {
            store_path,
            expected: format!("{} ({} bytes)", narinfo.nar_hash.to_sri(), narinfo.nar_size),
            found: format!("{} ({size} bytes)", hash.to_sri()),
        });
    }
    Ok(())
}

fn decompress(compression: Compression, compressed: ChunkReader) -> io::Result<Box<dyn Read>> {
    Ok(match compression {
        Compression::None => Box::new(compressed),
        Compression::Xz => Box::new(XzReader::new(compressed, true)),
        Compression::Zstd => Box::new(zstd::stream::read::Decoder::new(compressed)?),
    })
}

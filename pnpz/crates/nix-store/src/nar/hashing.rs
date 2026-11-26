use crate::NarHash;
use sha2::{
    Digest,
    Sha256,
};
use std::io::{
    self,
    Read,
};

/// Hashes and counts the bytes read through it, so an archive can be unpacked
/// and verified in one pass.
pub struct HashingReader<Inner> {
    inner: Inner,
    hasher: Sha256,
    byte_count: u64,
}

impl<Inner: Read> HashingReader<Inner> {
    pub fn new(inner: Inner) -> Self {
        Self { inner, hasher: Sha256::new(), byte_count: 0 }
    }

    /// Reads the rest of the input, then returns its hash and total size.
    ///
    /// Draining matters: an archive that parses completely but is followed by
    /// more bytes has a different hash than the tree that was unpacked.
    pub fn finish(mut self) -> io::Result<(NarHash, u64)> {
        io::copy(&mut self, &mut io::sink())?;
        Ok((NarHash::from_digest(self.hasher.finalize().into()), self.byte_count))
    }
}

impl<Inner: Read> Read for HashingReader<Inner> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        self.byte_count += read as u64;
        Ok(read)
    }
}

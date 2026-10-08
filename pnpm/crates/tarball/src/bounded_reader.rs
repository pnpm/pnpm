use std::io::{self, Read};

/// A stream that ends in an error once it has given out more than `limit`
/// bytes, for unpacking an archive whose compressed size does not bound
/// what it unpacks to.
pub struct BoundedReader<Stream> {
    inner: Stream,
    left: u64,
}

impl<Stream> BoundedReader<Stream> {
    pub fn new(inner: Stream, limit: u64) -> Self {
        Self { inner, left: limit }
    }
}

impl<Stream: Read> Read for BoundedReader<Stream> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.left = self.left
            .checked_sub(read as u64)
            .ok_or_else(|| io::Error::other("it unpacks to more than it may"))?;
        Ok(read)
    }
}

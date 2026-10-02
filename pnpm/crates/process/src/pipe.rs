use crate::{child::ProcessHandle, host};
use pnpm_wasm_host::Request;
use serde::Deserialize;
use serde_json::json;
use std::{
    future::Future,
    io::{self, Read, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct ReadPipe {
    handle: u32,
    pending: Option<Request>,
    buffered: Vec<u8>,
    offset: usize,
    done: bool,
}

impl ReadPipe {
    pub(crate) fn new(handle: u32) -> Self {
        Self { handle, pending: None, buffered: Vec::new(), offset: 0, done: false }
    }

    fn start_read(&mut self, length: usize) -> io::Result<()> {
        if self.pending.is_none() {
            self.pending = Some(
                pnpm_wasm_host::request(&json!({
                    "operation": "stream.read", "handle": self.handle,
                    "maxBytes": length.min(64 * 1024),
                }))
                .map_err(host::error)?,
            );
        }
        Ok(())
    }

    fn receive(&mut self, value: serde_json::Value) -> io::Result<()> {
        #[derive(Deserialize)]
        struct Chunk {
            bytes: Vec<u8>,
            done: bool,
        }
        let chunk: Chunk = serde_json::from_value(value).map_err(io::Error::other)?;
        self.buffered = chunk.bytes;
        self.offset = 0;
        self.done = chunk.done;
        Ok(())
    }

    fn copy_buffered(&mut self, output: &mut [u8]) -> usize {
        let remaining = &self.buffered[self.offset..];
        let count = remaining.len().min(output.len());
        output[..count].copy_from_slice(&remaining[..count]);
        self.offset += count;
        count
    }
}

impl Read for ReadPipe {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.offset == self.buffered.len() && !self.done {
            self.start_read(output.len())?;
            let pending = self.pending.take().expect("read request started");
            self.receive(host::block_on(pending).map_err(host::error)?)?;
        }
        Ok(self.copy_buffered(output))
    }
}

impl AsyncRead for ReadPipe {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let pipe = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if pipe.offset == pipe.buffered.len() && !pipe.done {
            pipe.start_read(output.remaining())?;
            let pending = pipe.pending.as_mut().expect("read request started");
            let response = std::task::ready!(Pin::new(pending).poll(context));
            pipe.pending = None;
            pipe.receive(response.map_err(host::error)?)?;
        }
        let copied = pipe.copy_buffered(output.initialize_unfilled());
        output.advance(copied);
        Poll::Ready(Ok(()))
    }
}

impl Drop for ReadPipe {
    fn drop(&mut self) {
        drop(self.pending.take());
        if let Err(error) = pnpm_wasm_host::close_resource(self.handle) {
            eprintln!("Failed to close child output: {error}");
        }
    }
}

/// Child input with one bounded host write in flight. Flush awaits delivery;
/// dropping closes stdin after accepted writes without waiting for the child.
pub struct WritePipe {
    process: Arc<ProcessHandle>,
    pending: Option<Request>,
    closing: Option<Request>,
    closed: bool,
}

impl WritePipe {
    pub(crate) fn new(process: Arc<ProcessHandle>) -> Self {
        Self { process, pending: None, closing: None, closed: false }
    }

    fn check_open(&self) -> io::Result<()> {
        if self.closed {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "Child stdin is closed"))
        } else {
            Ok(())
        }
    }

    fn flush_pending(&mut self) -> io::Result<()> {
        if let Some(request) = self.pending.take() {
            host::block_on(request).map_err(host::error)?;
        }
        Ok(())
    }
}

impl Write for WritePipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.check_open()?;
        self.flush_pending()?;
        let count = buffer.len().min(64 * 1024);
        host::request(
            &json!({"operation": "process.write", "handle": self.process.handle, "bytes": &buffer[..count]}),
        )?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.check_open()?;
        self.flush_pending()
    }
}

impl AsyncWrite for WritePipe {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        std::task::ready!(self.as_mut().poll_flush(context))?;
        let pipe = self.get_mut();
        let count = buffer.len().min(64 * 1024);
        if count == 0 {
            return Poll::Ready(Ok(0));
        }
        pipe.pending = Some(pnpm_wasm_host::request(&json!({
            "operation": "process.write", "handle": pipe.process.handle, "bytes": &buffer[..count],
        })).map_err(host::error)?);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let pipe = self.get_mut();
        pipe.check_open()?;
        if let Some(request) = &mut pipe.pending {
            let result = std::task::ready!(Pin::new(request).poll(context));
            pipe.pending = None;
            result.map_err(host::error)?;
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        std::task::ready!(self.as_mut().poll_flush(context))?;
        let pipe = self.get_mut();
        if pipe.closing.is_none() {
            pipe.closing = Some(
                pnpm_wasm_host::request(&json!({
                    "operation": "process.end", "handle": pipe.process.handle,
                }))
                .map_err(host::error)?,
            );
        }
        let request = pipe.closing.as_mut().expect("stdin shutdown started");
        let result = std::task::ready!(Pin::new(request).poll(context));
        pipe.closing = None;
        if result.is_ok() {
            pipe.closed = true;
        }
        Poll::Ready(result.map(|_| ()).map_err(host::error))
    }
}

impl Drop for WritePipe {
    fn drop(&mut self) {
        if !self.closed
            && let Err(error) = host::request(&json!({
                "operation": "process.endDetached", "handle": self.process.handle,
            }))
        {
            eprintln!("Failed to close child stdin: {error}");
        }
    }
}

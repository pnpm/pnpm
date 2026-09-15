//! Stable content hash of an in-memory [`Lockfile`].
//!
//! Used by the verification cache to recognise the
//! same lockfile across paths — committed-then-restored CI checkouts,
//! parallel git worktrees, lockfile copies. The same parsed
//! [`Lockfile`] must yield the same hash every time regardless of
//! how the underlying YAML was ordered when written.
//!
//! Upstream uses `@pnpm/crypto.object-hasher`'s `hashObject` (a
//! sha256-base64 streamed through the `object-hash` npm package with
//! `unorderedObjects: true`). Pacquet's implementation is functionally
//! equivalent but format-divergent: stream the lockfile through
//! `serde_json` with every map normalized to sorted key order, hash
//! the bytes with sha256, output **hex** (not base64). Cross-stack
//! cache hits are not expected — each stack reads its own records out
//! of the shared JSONL — and the per-stack determinism is what the
//! cache contract actually requires.

use std::io;

use pnpm_lockfile::Lockfile;
use sha2::{Digest, Sha256};

/// Sha256 hex digest of the lockfile content.
///
/// The same on-write normalization the writer applies runs first, so an
/// in-memory lockfile hashes to what it will hash to once saved and read
/// back — which is what lets an install record the verification its
/// successor looks up.
#[must_use]
pub fn hash_lockfile(lockfile: &Lockfile) -> String {
    let mut value = serde_json::to_value(lockfile)
        .expect("Lockfile serializes; serde_json::Value supports all JSON-shape variants");
    pnpm_lockfile::prune_time(&mut value);
    value.sort_all_objects();
    let mut hasher = HashWriter(Sha256::new());
    serde_json::to_writer(&mut hasher, &value)
        .expect("HashWriter is infallible; serde_json::to_writer cannot fail otherwise");
    format!("{:x}", hasher.0.finalize())
}

/// `io::Write` adapter that feeds bytes into a [`Sha256`] as they
/// arrive from `serde_json::to_writer`, so the full normalized JSON
/// never materializes in memory. Mirrors upstream's streaming
/// behavior — the lockfile can be megabytes for large monorepos.
struct HashWriter(Sha256);

impl io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;

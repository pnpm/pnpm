//! Stable content hash of an in-memory [`Lockfile`].
//!
//! Used by the verification cache (Phase 6 slice 2) to recognise the
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

use pacquet_lockfile::Lockfile;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Sha256 hex digest of the lockfile content. Stable across runs:
/// any two `Lockfile`s that compare equal (deserialized from the
/// same YAML, or from two YAMLs that parse to the same shape)
/// produce the same hash.
#[must_use]
pub fn hash_lockfile(lockfile: &Lockfile) -> String {
    let value = serde_json::to_value(lockfile)
        .expect("Lockfile serializes; serde_json::Value supports all JSON-shape variants");
    let normalized = normalize(value);
    let mut hasher = HashWriter(Sha256::new());
    serde_json::to_writer(&mut hasher, &normalized)
        .expect("HashWriter is infallible; serde_json::to_writer cannot fail otherwise");
    format!("{:x}", hasher.0.finalize())
}

/// Walk a [`Value`] and rebuild every map with sorted keys. Arrays
/// are left in place — the lockfile's only array-shaped fields are
/// `ignoredOptionalDependencies` (which is semantically a set the
/// install treats as ordered for diff stability) and dependency-name
/// lists inside individual entries (where order matches the manifest
/// section it came from).
///
/// `serde_json::Map` is backed by `IndexMap` under the
/// `preserve_order` workspace feature, so a fresh `Map` populated in
/// sorted-key order serialises in that order.
fn normalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let mut sorted = Map::with_capacity(keys.len());
            for key in keys {
                let inner =
                    map.get(&key).cloned().expect("key came from the same map we're walking");
                sorted.insert(key, normalize(inner));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
        other => other,
    }
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

//! Hash helpers shared across pacquet crates.
//!
//! Also hosts [`shorten_virtual_store_name`], the trailing
//! length/case-shortening branch of the depPath-to-filename encoding:
//! the lockfile and registry helpers both need to apply the same
//! shortening after their own pre-escape step. Keeping the helpers in
//! one place avoids duplicating the sha2 dependency in every consumer
//! (lockfile, registry, store-dir).

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use std::{io, path::Path};

/// Compute the `sha256-<base64>` digest of `input`.
///
/// Produces `` `sha256-${base64}` ``. This is the shape pnpm writes for
/// `pnpmfileChecksum` and (via the object hasher)
/// `packageExtensionsChecksum`.
#[must_use]
pub fn create_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("sha256-{}", BASE64.encode(digest))
}

/// Read `path` as UTF-8, normalize CRLF line endings to LF, and hash the
/// result with [`create_hash`].
pub fn create_hash_from_file(path: &Path) -> io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    Ok(create_hash(&content.replace("\r\n", "\n")))
}

/// Compute the full sha256 hex digest of `input`.
///
/// Used for the global-install cache key (`createGlobalCacheKey`),
/// whose value names an on-disk symlink, so the full hex digest is part
/// of the directory layout.
#[must_use]
pub fn create_hex_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("{digest:x}")
}

/// Compute the sha256 hex digest of `input` and truncate to the first
/// 32 hex characters (16 bytes of entropy).
///
/// The truncation is part of the on-disk contract — anything written
/// into a path with this hash (project-registry slugs, virtual-store
/// dirnames that overflowed `virtualStoreDirMaxLength`, etc.) must use
/// the same 32-char length so pacquet and pnpm produce the same
/// directory layout.
#[must_use]
pub fn create_short_hash(input: &str) -> String {
    let mut hex = create_hex_hash(input);
    hex.truncate(32);
    hex
}

/// Hash-shorten `filename` when it exceeds `max_length` bytes or carries
/// uppercase characters that would collide on case-insensitive
/// filesystems. Returns `filename` unchanged otherwise.
///
/// When shortening is needed, the result is the first
/// `max_length - 33` bytes of `filename` followed by `_` and the
/// [`create_short_hash`] of the full `filename`. Names that already
/// start with `file+` are exempt from the case check.
///
/// `max_length` is `Modules.virtual_store_dir_max_length` (default
/// 120; see `pacquet_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH`).
///
/// The caller is responsible for pre-escaping the source string (parens
/// → underscores, scoped-name slashes → `+`, etc) — this helper only
/// applies the final length/case decision so the escape rules can stay
/// where the structured input lives.
#[must_use]
pub fn shorten_virtual_store_name(filename: String, max_length: usize) -> String {
    let lower = filename.to_ascii_lowercase();
    let needs_shortening =
        filename.len() > max_length || (filename != lower && !filename.starts_with("file+"));
    if !needs_shortening {
        return filename;
    }
    let cap = max_length.saturating_sub(33);
    let mut boundary = cap.min(filename.len());
    while !filename.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let hash = create_short_hash(&filename);
    format!("{}_{}", &filename[..boundary], hash)
}

#[cfg(test)]
mod tests;

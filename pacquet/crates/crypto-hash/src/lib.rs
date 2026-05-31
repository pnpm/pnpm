//! Hash helpers shared across pacquet crates.
//!
//! Mirrors upstream pnpm's
//! [`@pnpm/crypto.hash`](https://github.com/pnpm/pnpm/blob/1819226b51/crypto/hash/src/index.ts)
//! package. Also hosts [`shorten_virtual_store_name`], the trailing
//! length/case-shortening branch of upstream's
//! [`depPathToFilename`](https://github.com/pnpm/pnpm/blob/1819226b51/deps/path/src/index.ts#L169-L180):
//! pacquet doesn't have a `deps.path`-equivalent crate yet, and the
//! lockfile and registry helpers both need to apply the same shortening
//! after their own pre-escape step. Keeping the helpers in one place
//! avoids duplicating the sha2 dependency in every consumer (lockfile,
//! registry, store-dir).

use sha2::{Digest, Sha256};

/// Compute the sha256 hex digest of `input` and truncate to the first
/// 32 hex characters (16 bytes of entropy).
///
/// Matches upstream
/// [`createShortHash`](https://github.com/pnpm/pnpm/blob/1819226b51/crypto/hash/src/index.ts#L7-L9):
/// `crypto.hash('sha256', input, 'hex').substring(0, 32)`. The truncation
/// is part of the on-disk contract — anything written into a path with
/// this hash (project-registry slugs, virtual-store dirnames that
/// overflowed `virtualStoreDirMaxLength`, etc.) must use the same 32-char
/// length so pacquet and pnpm produce the same directory layout.
pub fn create_short_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut hex = format!("{digest:x}");
    hex.truncate(32);
    hex
}

/// Hash-shorten `filename` when it exceeds `max_length` bytes or carries
/// uppercase characters that would collide on case-insensitive
/// filesystems. Returns `filename` unchanged otherwise.
///
/// Mirrors the trailing branch of upstream's
/// [`depPathToFilename`](https://github.com/pnpm/pnpm/blob/1819226b51/deps/path/src/index.ts#L169-L180):
///
/// ```text
/// if (filename.length > maxLengthWithoutHash ||
///     (filename !== filename.toLowerCase() && !filename.startsWith('file+'))) {
///     return `${filename.substring(0, maxLengthWithoutHash - 33)}_${createShortHash(filename)}`
/// }
/// ```
///
/// `max_length` is `Modules.virtual_store_dir_max_length` (default
/// 120; see `pacquet_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH`).
/// The `file+` early exit keeps file-protocol deps from hashing just
/// because their on-disk path component carries capitals.
///
/// The caller is responsible for pre-escaping the source string (parens
/// → underscores, scoped-name slashes → `+`, etc) — this helper only
/// applies the final length/case decision so the escape rules can stay
/// where the structured input lives.
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

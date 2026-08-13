//! On-disk cache for per-version runtime `SHASUMS256.txt` bodies.
//!
//! A release's SHASUMS file lives under a version-pinned URL
//! (`.../v22.0.0/SHASUMS256.txt`), so its content is immutable: a body
//! fetched once — and, for signed channels, verified once — never needs
//! to be fetched again. Entries are stored under
//! `<cache_dir>/v11/runtime-shasums/<host>/<url path>` so `pnpm cache
//! delete` on the cache directory clears them together with the
//! registry metadata mirror. The layout is shared with pnpm, which
//! reads and writes the same files.
//!
//! Only hand immutable URLs to this cache. A cached body is trusted on
//! the same terms as the registry metadata mirror: verification (the
//! `OpenPGP` signature check for signed channels) happens before the
//! write, not after the read, so a reader never re-verifies.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// Directory under the pnpm cache dir holding the cached SHASUMS
/// bodies. The `v11/` prefix groups it with the registry metadata
/// mirror dirs (`v11/metadata`, ...), which share the cache dir's
/// versioning story.
pub const RUNTIME_SHASUMS_CACHE_DIR: &str = "v11/runtime-shasums";

/// The cache file backing `url`, or `None` when the URL has a shape
/// the path mapping does not cover (non-HTTP scheme, embedded
/// credentials, query string, empty or dot-only path segments).
/// Returning `None` just disables caching for that URL.
pub(crate) fn shasums_cache_path(cache_dir: &Path, url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    if rest.contains(['?', '#', '@']) {
        return None;
    }
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    // `:` (a port separator) is not portable in file names; `+` is the
    // same encoding the registry metadata mirror uses for it.
    let host = host.to_ascii_lowercase().replace(':', "+");
    let mut segments = vec![encode_path_segment(&host)?];
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        segments.push(encode_path_segment(segment)?);
    }
    let mut file = cache_dir.join(RUNTIME_SHASUMS_CACHE_DIR);
    file.extend(segments);
    Some(file)
}

/// Percent-encode the bytes of `segment` that are not portable across
/// filesystems, keeping `[A-Za-z0-9._+-]` as-is. pnpm applies the same
/// encoding, so both tools address one file.
fn encode_path_segment(segment: &str) -> Option<String> {
    if segment.len() > 200 {
        return None;
    }
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-') {
            encoded.push(byte as char);
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    Some(encoded)
}

/// The cached body for `url`, or `None` on any miss — an unmappable
/// URL, a missing file, unreadable content, or an empty file (never a
/// valid SHASUMS body, so it only signals a torn write).
pub(crate) fn read_cached_shasums(cache_dir: Option<&Path>, url: &str) -> Option<String> {
    let path = shasums_cache_path(cache_dir?, url)?;
    fs::read_to_string(path).ok().filter(|body| !body.is_empty())
}

/// Best-effort write of `body` for `url`: a cache-write failure only
/// costs a refetch on the next resolve, so errors are deliberately
/// dropped rather than failing the resolution that produced the body.
/// The temp-file + rename keeps concurrent writers (two installs
/// resolving the same version) from exposing a torn body.
pub(crate) fn write_cached_shasums(cache_dir: Option<&Path>, url: &str, body: &str) {
    let Some(cache_dir) = cache_dir else { return };
    let Some(path) = shasums_cache_path(cache_dir, url) else { return };
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(format!(".tmp{}", std::process::id()));
    let temp = path.with_file_name(temp_name);
    if fs::write(&temp, body).is_ok() && fs::rename(&temp, &path).is_err() {
        let _ = fs::remove_file(&temp);
    }
}

#[cfg(test)]
mod tests;

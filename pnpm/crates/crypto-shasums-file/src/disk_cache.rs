//! On-disk cache for per-version runtime `SHASUMS256.txt` bodies.
//!
//! A release's SHASUMS file lives under a version-pinned URL
//! (`.../v22.0.0/SHASUMS256.txt`), so its content is immutable: a body
//! fetched once — and, for signed channels, verified once — never needs
//! to be fetched again. Entries are stored under
//! `<cache_dir>/v11/runtime-shasums/<trust>/<host>/<url path>` so
//! `pnpm cache delete` on the cache directory clears them together with
//! the registry metadata mirror. The layout is shared with pnpm, which
//! reads and writes the same files.
//!
//! Only hand immutable URLs to this cache. The cache directory is
//! *project-configurable* (`cacheDir`), so entries under it must never
//! carry more authority than the project that could have written them:
//!
//! - Signed-channel readers persist the detached signature next to the
//!   body and re-verify it against the embedded release keys on every
//!   read (see `fetch_verified_node_shasums_file_cached`), so a
//!   pre-seeded entry is only accepted if it is a genuine release
//!   body.
//! - Unverified entries are trusted on read, which grants a project
//!   nothing new: the unsigned channels' mirrors are already
//!   project-configurable (their bodies were project-controllable
//!   before the cache existed), and the musl list's download URLs are
//!   derived from the hardcoded unofficial-builds base, never from the
//!   cached body.
//!
//! The `<trust>` path segment keeps the two classes in disjoint
//! subtrees so their read policies cannot be confused.

use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Directory under the pnpm cache dir holding the cached SHASUMS
/// bodies. The `v11/` prefix groups it with the registry metadata
/// mirror dirs (`v11/metadata`, ...), which share the cache dir's
/// versioning story.
pub const RUNTIME_SHASUMS_CACHE_DIR: &str = "v11/runtime-shasums";

/// How the body of a cache entry was authenticated before it was
/// written. Each class caches into its own subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShasumsTrust {
    /// The body's detached `OpenPGP` signature verified against the
    /// embedded release keys.
    Verified,
    /// The body is trusted only as far as the TLS fetch that produced
    /// it (unsigned channels, unofficial-builds musl lists).
    Unverified,
}

impl ShasumsTrust {
    fn dir_name(self) -> &'static str {
        match self {
            ShasumsTrust::Verified => "verified",
            ShasumsTrust::Unverified => "unverified",
        }
    }
}

/// Upper bound on a cache entry's size. Real SHASUMS bodies are a few
/// kilobytes; anything past this bound is not a release asset list and
/// is never read into memory or written.
const MAX_CACHED_SHASUMS_LEN: u64 = 1024 * 1024;

/// The cached body for `url` as UTF-8 text, via [`read_cached_bytes`].
pub(crate) fn read_cached_shasums(
    cache_dir: Option<&Path>,
    trust: ShasumsTrust,
    url: &str,
) -> Option<String> {
    String::from_utf8(read_cached_bytes(cache_dir, trust, url)?).ok()
}

/// The cached bytes for `url`, or `None` on any miss — a URL the
/// mapping cannot represent, a missing or non-regular file, unreadable
/// content, an empty file (never a valid entry, so it only signals a
/// torn write), or a file over [`MAX_CACHED_SHASUMS_LEN`].
pub(crate) fn read_cached_bytes(
    cache_dir: Option<&Path>,
    trust: ShasumsTrust,
    url: &str,
) -> Option<Vec<u8>> {
    use std::io::Read as _;

    let path = shasums_cache_path(cache_dir?, trust, url)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    // A FIFO planted at the entry path would block a plain `open` until
    // a writer appears; `O_NONBLOCK` makes the open return immediately
    // and is a no-op for regular files. (Windows directory entries
    // cannot be named pipes, so the plain open is safe there.)
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(&path).ok()?;
    // Checked on the opened handle, so nothing can swap the regular
    // file for a special one between check and read.
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    // A bounded reader rather than a metadata check keeps the cap
    // race-free: at most one byte past the bound is ever read,
    // whatever the file's size becomes between open and read.
    let mut body = Vec::new();
    let bytes_read = file.take(MAX_CACHED_SHASUMS_LEN + 1).read_to_end(&mut body).ok()?;
    (bytes_read > 0 && bytes_read as u64 <= MAX_CACHED_SHASUMS_LEN).then_some(body)
}

/// Best-effort write of `body` for `url`: a cache-write failure only
/// costs a refetch on the next resolve, so errors are deliberately
/// dropped rather than failing the resolution that produced the body.
/// The exclusively-created temp file + rename keeps concurrent writers
/// (two installs resolving the same version) from exposing a torn body.
pub(crate) fn write_cached_shasums(
    cache_dir: Option<&Path>,
    trust: ShasumsTrust,
    url: &str,
    body: &[u8],
) {
    let Some(cache_dir) = cache_dir else { return };
    if body.len() as u64 > MAX_CACHED_SHASUMS_LEN {
        return;
    }
    let Some(path) = shasums_cache_path(cache_dir, trust, url) else { return };
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    // The process id alone does not make the temp name unique (worker
    // threads and concurrent tasks share it), so a process-wide counter
    // joins it. `create_new` refuses to open a path that already exists
    // — a colliding writer or a pre-seeded symlink fails the open
    // instead of being followed — and any failure just skips the write.
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    let temp = path.with_file_name(temp_name);
    // `sync_all` before the rename so a crash cannot persist the
    // renamed name pointing at partially-written content — a torn
    // SHASUMS prefix still parses and would otherwise be served
    // (missing platform rows) until the cache is cleared.
    let written =
        fs::OpenOptions::new().write(true).create_new(true).open(&temp).and_then(|mut file| {
            file.write_all(body)?;
            file.sync_all()
        });
    if written.is_err() || fs::rename(&temp, &path).is_err() {
        let _ = fs::remove_file(&temp);
    }
}

/// The cache file backing `url`, or `None` when the URL has a shape
/// the path mapping does not cover (non-HTTP scheme, embedded
/// credentials, query string, empty or dot-only path segments).
/// Returning `None` just disables caching for that URL.
pub(crate) fn shasums_cache_path(
    cache_dir: &Path,
    trust: ShasumsTrust,
    url: &str,
) -> Option<PathBuf> {
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
    let mut file = cache_dir.join(RUNTIME_SHASUMS_CACHE_DIR).join(trust.dir_name());
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

#[cfg(test)]
mod tests;

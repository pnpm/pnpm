//! On-disk packument-mirror helpers.
//!
//! Ports the cache-path and IO helpers in pnpm's
//! [`pickPackage.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts)
//! the verifier needs to share the resolver's metadata mirror:
//!
//! - [`get_pkg_mirror_path`] — `<cache_dir>/<meta_dir>/<registry-encoded>/<encoded-name>.jsonl`.
//! - [`prepare_json_for_disk`] — two-line NDJSON shape (header line +
//!   body line) the registry-metadata cache uses.
//! - [`load_meta_headers`] — read just the first line (etag, modified)
//!   to feed conditional GETs without paying for the body parse.
//! - [`load_meta`] — read both lines and reconstruct a [`Package`]
//!   with its etag back-filled.
//! - [`save_meta`] — atomic write via temp + rename so a torn write
//!   never leaks a half-formed mirror to the next install.
//!
//! Plus the constants and name-encoding rules:
//!
//! - [`FULL_META_DIR`] / [`ABBREVIATED_META_DIR`] — directory slugs
//!   pnpm and pacquet share.
//! - [`encode_pkg_name`] — mixed-case package names get a sha256 hex
//!   suffix so case-insensitive filesystems (HFS+, NTFS by default)
//!   can't collide two distinct package names onto one mirror file.
//! - [`get_registry_name`] — `host[:port]` with `:` → `+` (the
//!   filesystem-safe encoding the npm `encode-registry` package
//!   produces).

use std::{
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_registry::Package;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Mirror directory for the **abbreviated** metadata cache. Mirrors
/// upstream's
/// [`ABBREVIATED_META_DIR`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/core/constants/src/index.ts#L21).
pub const ABBREVIATED_META_DIR: &str = "v11/metadata";

/// Mirror directory for the **full** metadata cache. Mirrors
/// upstream's
/// [`FULL_META_DIR`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/core/constants/src/index.ts#L22).
pub const FULL_META_DIR: &str = "v11/metadata-full";

/// Cached headers persisted as the mirror's first line. The cached
/// metadata fetcher feeds these into `If-None-Match` /
/// `If-Modified-Since` on the next request. Both fields are
/// optional because some registries omit one or the other; the
/// fetcher tolerates a partial header set and only sends the headers
/// it has.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaHeaders {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// Error from [`save_meta`]. Surfaced to callers that care about
/// individual write failures (tests, in particular); production
/// callers ignore it and treat cache writes as fire-and-forget.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SaveMetaError {
    #[display("Failed to create mirror directory {dir:?}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::mirror::create_dir))]
    CreateDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to write mirror temp file {temp:?}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::mirror::write_temp))]
    WriteTemp {
        temp: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to rename mirror temp {temp:?} → {target:?}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::mirror::rename))]
    Rename {
        temp: PathBuf,
        target: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// On-disk path of the JSONL document where pacquet (and pnpm)
/// mirrors a package's registry metadata. Matches pnpm's
/// [`getPkgMirrorPath`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L566-L568).
pub fn get_pkg_mirror_path(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    pkg_name: &str,
) -> Result<PathBuf, EncodeRegistryError> {
    let registry_name = get_registry_name(registry)?;
    let encoded_name = encode_pkg_name(pkg_name);
    Ok(cache_dir.join(meta_dir).join(registry_name).join(format!("{encoded_name}.jsonl")))
}

/// Failure parsing a registry URL into a filesystem-safe slug.
/// Real-world registries always carry a host; this only triggers on
/// malformed config.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EncodeRegistryError {
    #[display("Failed to parse registry URL {url:?}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::mirror::parse_registry))]
    ParseUrl {
        #[error(not(source))]
        url: String,
        error: String,
    },
    #[display("Registry URL {url:?} has no host")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::mirror::missing_host))]
    MissingHost {
        #[error(not(source))]
        url: String,
    },
}

/// `host[:port]` form of a registry URL with `:` rewritten to `+` so
/// the result is filesystem-safe. Mirrors the npm
/// [`encode-registry`](https://github.com/zkochan/packages/tree/main/encode-registry)
/// package pnpm consumes — `https://npm.example:8443/` becomes
/// `npm.example+8443`, `https://registry.npmjs.org/` becomes
/// `registry.npmjs.org`. Only an explicit port participates; the
/// implicit-default port stays out of the slug so a registry served
/// on its scheme default hashes consistently across configs.
pub fn get_registry_name(registry: &str) -> Result<String, EncodeRegistryError> {
    let parsed = reqwest::Url::parse(registry).map_err(|error| EncodeRegistryError::ParseUrl {
        url: registry.to_string(),
        error: error.to_string(),
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| EncodeRegistryError::MissingHost { url: registry.to_string() })?;
    Ok(match parsed.port() {
        Some(port) => format!("{host}+{port}"),
        None => host.to_string(),
    })
}

/// Filesystem-safe form of a package name. A mixed-case name (e.g.
/// `LRUCache`) gets a sha256 hex suffix so case-insensitive
/// filesystems (HFS+, NTFS by default) can't collide it with a
/// lowercase sibling. Mirrors pnpm's
/// [`encodePkgName`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L555-L560).
pub fn encode_pkg_name(pkg_name: &str) -> String {
    let lowered = pkg_name.to_lowercase();
    if pkg_name == lowered {
        return pkg_name.to_string();
    }
    let digest = Sha256::digest(pkg_name.as_bytes());
    format!("{pkg_name}_{digest:x}")
}

/// Serialize the cache record for disk. Two-line NDJSON: the first
/// line is the [`MetaHeaders`] JSON, the second is the registry
/// response body — verbatim when the caller supplies `raw_body`, else
/// re-serialized from the parsed `meta`. Mirrors pnpm's
/// [`prepareJsonForDisk`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L575-L580).
///
/// Passing `raw_body` is the fast path on a 200 response where the
/// caller already has the response bytes — round-tripping through
/// the parsed shape risks dropping registry-specific fields the
/// pacquet `Package` shape doesn't model (and would diff a later
/// install's cache against pnpm's byte-for-byte). The `meta` /
/// `raw_body` fallback is only for tests that build the record from
/// a typed value.
pub fn prepare_json_for_disk(
    meta: &Package,
    etag: Option<&str>,
    raw_body: Option<&str>,
) -> Result<String, serde_json::Error> {
    let modified = meta.modified.clone();
    let headers = serde_json::to_string(&MetaHeaders { etag: etag.map(str::to_string), modified })?;
    let body = match raw_body {
        Some(text) => text.to_string(),
        None => serde_json::to_string(meta)?,
    };
    Ok(format!("{headers}\n{body}"))
}

/// Read just the first line (headers JSON) of a mirror file. The
/// fetcher uses this to issue a conditional GET without paying the
/// full-body parse cost on a warm cache.
///
/// Returns `None` on any failure — missing file, unreadable header
/// line, parse error. The fetcher then proceeds without conditional
/// headers, identical to pnpm's
/// [`loadMetaHeaders`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L627-L644)
/// catch-and-return-null.
pub fn load_meta_headers(pkg_mirror: &Path) -> Option<MetaHeaders> {
    let mut file = File::open(pkg_mirror).ok()?;
    // Upstream uses a 1 KB buffer; the headers JSON is typically
    // ~100 bytes. We match the upstream choice so a hand-edited
    // mirror behaves the same on both stacks.
    let mut buf = [0u8; 1024];
    let bytes_read = file.read(&mut buf).ok()?;
    if bytes_read == 0 {
        return None;
    }
    let chunk = &buf[..bytes_read];
    let newline = chunk.iter().position(|&b| b == b'\n')?;
    let line = std::str::from_utf8(&chunk[..newline]).ok()?;
    serde_json::from_str(line).ok()
}

/// Read the full mirror file and reconstruct a [`Package`] with its
/// etag back-filled from the headers line. Mirrors pnpm's
/// [`loadMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L651-L663).
///
/// Returns `None` on missing file / malformed contents. Upstream
/// catches any error from `readFile` / `JSON.parse` and returns
/// `null`; we match that contract because the caller's response to
/// "couldn't read" is the same as "no cache".
pub fn load_meta(pkg_mirror: &Path) -> Option<Package> {
    let data = fs::read_to_string(pkg_mirror).ok()?;
    let newline = data.find('\n')?;
    let headers: MetaHeaders = serde_json::from_str(&data[..newline]).ok()?;
    let mut meta: Package = serde_json::from_str(&data[newline + 1..]).ok()?;
    meta.etag = headers.etag;
    Some(meta)
}

/// Async sibling of [`load_meta`]. The body is a blocking
/// `fs::read_to_string` plus a `serde_json::from_str` that can chew
/// through a multi-KB to multi-MB packument body — neither yields, so
/// calling [`load_meta`] directly from an async task on the resolve
/// hot path blocks the tokio worker for the duration of the read +
/// parse. With hundreds of unique packuments per install, that
/// serializes the resolve walk against the size of the runtime's
/// worker pool. This wrapper dispatches the work to
/// [`tokio::task::spawn_blocking`] so the async scheduler keeps
/// progressing other resolves and HTTP fetches while one packument's
/// body parses on the blocking pool. Matches upstream's stance:
/// pnpm's loadMeta is an awaited `fs.readFile` + `JSON.parse` that
/// runs on libuv's worker pool, the same separation tokio gives us
/// via `spawn_blocking`.
///
/// `JoinError` (panic in the blocking task) and `None` from
/// [`load_meta`] (missing / unreadable file) both collapse to
/// `None`. The caller's response to either is the same — fall
/// through to the network fetch — so distinguishing them is not
/// load-bearing.
///
/// Returns `None` immediately when `pkg_mirror` is `None`, skipping
/// the spawn-blocking dispatch entirely on the no-cache-dir branch.
pub async fn load_meta_async(pkg_mirror: Option<&Path>) -> Option<Package> {
    let pkg_mirror = pkg_mirror?.to_path_buf();
    tokio::task::spawn_blocking(move || load_meta(&pkg_mirror)).await.ok().flatten()
}

/// Async sibling of [`load_meta_headers`]. Same rationale as
/// [`load_meta_async`] — the synchronous body opens a file and
/// parses a short JSON header line, blocking the worker for the
/// duration. The headers-only read is cheap (~100 bytes typically)
/// but is invoked on every cache-warm pick, so the cumulative block
/// time is still meaningful with hundreds of packuments.
pub async fn load_meta_headers_async(pkg_mirror: Option<&Path>) -> Option<MetaHeaders> {
    let pkg_mirror = pkg_mirror?.to_path_buf();
    tokio::task::spawn_blocking(move || load_meta_headers(&pkg_mirror)).await.ok().flatten()
}

/// Atomic write: serialize to a sibling temp file, then `rename` it
/// over the target. Mirrors pnpm's
/// [`saveMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L667-L676).
///
/// The rename is the only atomic step; an observer sees either the
/// old contents or the new ones, never a torn body line.
pub fn save_meta(pkg_mirror: &Path, json: &str) -> Result<(), SaveMetaError> {
    let dir = pkg_mirror.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)
        .map_err(|error| SaveMetaError::CreateDir { dir: dir.to_path_buf(), error })?;
    let temp = temp_sibling_path(pkg_mirror);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| SaveMetaError::WriteTemp { temp: temp.clone(), error })?;
        file.write_all(json.as_bytes())
            .map_err(|error| SaveMetaError::WriteTemp { temp: temp.clone(), error })?;
    }
    fs::rename(&temp, pkg_mirror).map_err(|error| {
        // Best-effort cleanup so a stale temp doesn't accumulate on
        // a rename failure (e.g. cross-device move on an unusual mount).
        let _ = fs::remove_file(&temp);
        SaveMetaError::Rename { temp, target: pkg_mirror.to_path_buf(), error }
    })?;
    Ok(())
}

/// Per-process atomic counter used to disambiguate concurrent
/// `save_meta` calls writing to sibling temp paths under the same
/// mirror directory. Pid + counter is enough — pnpm's pathTemp uses
/// the same shape (`<pid>.<counter>` suffix) for the same reason.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_sibling_path(target: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut name = match target.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_string(),
        None => "tmp".to_string(),
    };
    write!(name, ".{pid}.{counter}.tmp").unwrap();
    target.with_file_name(name)
}

#[cfg(test)]
mod tests;

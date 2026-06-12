//! On-disk packument-mirror helpers.
//!
//! Ports the cache-path and IO helpers in pnpm's
//! [`pickPackage.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts)
//! the verifier needs to share the resolver's metadata mirror:
//!
//! - [`get_pkg_mirror_path`] — `<cache_dir>/<meta_dir>/<registry-encoded>/<encoded-name>.jsonl`.
//! - [`load_meta_headers`] — read just the headers record (etag,
//!   modified) to feed conditional GETs without touching the rest.
//! - [`load_meta`] — read the headers + index records and reconstruct
//!   a [`Package`] whose versions hydrate from byte spans on demand.
//! - [`save_meta_indexed`] — atomic write via temp + rename so a torn
//!   write never leaks a half-formed mirror to the next install.
//!
//! ## File layout
//!
//! Pacquet's own indexed format (this cache is no longer byte-shared
//! with other package managers):
//!
//! ```text
//! pacquet-meta-v1 <headers_len> <index_len>\n
//! <headers JSON>           # MetaHeaders: etag, modified
//! <index JSON>             # MirrorIndex: name, dist-tags, time,
//!                          #   homepage, versions: [version, off, len]
//! <fragments>              # concatenated raw per-version JSON
//! ```
//!
//! Offsets in the index are relative to the fragment section; the
//! loader rebases them so each version's slot can read its span
//! directly. A warm pick therefore costs the two leading records plus
//! one span read per version it actually hydrates — never the whole
//! body. Files in the older two-line NDJSON shape read as cache
//! misses and are rewritten in this format on the next 200.
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
    collections::HashMap,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_registry::{Package, PackageVersions};
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
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Encode(#[error(source)] EncodeMetaError),
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

/// Magic + format version. The trailing space separates it from the
/// two record lengths on the same line.
const MIRROR_MAGIC: &str = "pacquet-meta-v1";

/// Top-level packument fields persisted in the mirror's index record.
/// Everything else a registry serves at the top level is neither read
/// back by the resolver nor part of [`Package`], so the index keeps
/// only what reconstruction needs. Version fragments live after this
/// record as `(version, offset, len)` spans relative to the fragment
/// section.
#[derive(Debug, Serialize, Deserialize)]
struct MirrorIndex {
    name: String,
    #[serde(default, rename = "distTags")]
    dist_tags: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    homepage: Option<String>,
    versions: Vec<(String, u64, u32)>,
}

/// Error from [`save_meta_indexed`]'s record-encoding step.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to encode mirror records: {_0}")]
#[diagnostic(code(pacquet_resolving_npm_resolver::mirror::encode))]
pub struct EncodeMetaError(#[error(source)] serde_json::Error);

/// Atomically persist `meta` at `pkg_mirror` in the indexed format.
///
/// Version fragments come from [`PackageVersions::fragments`] — for a
/// freshly-fetched packument these borrow the raw bytes the registry
/// served, so the write is one buffered pass with no re-serialization.
/// The cold-install cost is therefore the same one temp-file +
/// `rename` per package as the previous format.
pub fn save_meta_indexed(
    pkg_mirror: &Path,
    meta: &Package,
    etag: Option<&str>,
) -> Result<(), SaveMetaError> {
    let headers = serde_json::to_string(&MetaHeaders {
        etag: etag.map(str::to_string),
        modified: meta.modified.clone(),
    })
    .map_err(|error| SaveMetaError::Encode(EncodeMetaError(error)))?;

    let mut fragment_bytes = Vec::new();
    let mut spans = Vec::with_capacity(meta.versions.len());
    for (version, json) in meta.versions.fragments() {
        let offset = fragment_bytes.len() as u64;
        let len = u32::try_from(json.len()).unwrap_or(u32::MAX);
        if len as usize != json.len() {
            // A single >4 GiB version manifest is not a thing the npm
            // registry produces; skip it rather than corrupt the index.
            continue;
        }
        fragment_bytes.extend_from_slice(json.as_bytes());
        spans.push((version.clone(), offset, len));
    }

    let index = serde_json::to_string(&MirrorIndex {
        name: meta.name.clone(),
        dist_tags: meta.dist_tags.clone(),
        time: meta.time.clone(),
        homepage: meta.homepage.clone(),
        versions: spans,
    })
    .map_err(|error| SaveMetaError::Encode(EncodeMetaError(error)))?;

    let mut contents = String::with_capacity(headers.len() + index.len() + 64);
    let _ = writeln!(contents, "{MIRROR_MAGIC} {} {}", headers.len(), index.len());
    contents.push_str(&headers);
    contents.push_str(&index);
    let mut bytes = contents.into_bytes();
    bytes.extend_from_slice(&fragment_bytes);
    save_meta(pkg_mirror, &bytes)
}

/// Parse the `pacquet-meta-v1 <headers_len> <index_len>` line.
/// `None` for anything else — including the previous NDJSON format,
/// which thereby reads as a cache miss and gets rewritten on the next
/// 200 response.
fn parse_mirror_magic(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix(MIRROR_MAGIC)?.strip_prefix(' ')?;
    let (headers_len, index_len) = rest.split_once(' ')?;
    Some((headers_len.parse().ok()?, index_len.parse().ok()?))
}

/// Read the headers record off an indexed mirror without touching the
/// index or fragment sections. `None` for anything unreadable —
/// including the previous NDJSON format, which thereby reads as a
/// cache miss.
fn read_mirror_headers(file: &mut File) -> Option<MetaHeaders> {
    // Magic + two decimal lengths fit well inside this; the headers
    // record is ~100 bytes of etag + timestamp.
    let mut buf = [0u8; 1024];
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).ok()?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    let chunk = &buf[..filled];
    let newline = chunk.iter().position(|&byte| byte == b'\n')?;
    let line = std::str::from_utf8(&chunk[..newline]).ok()?;
    let (headers_len, _) = parse_mirror_magic(line)?;
    // The headers record is ~100 bytes of etag + timestamp. Bound the
    // declared length before allocating from it so a corrupted or
    // hostile mirror can't trigger an arbitrarily large allocation.
    const MAX_HEADERS_LEN: usize = 64 * 1024;
    if headers_len > MAX_HEADERS_LEN {
        return None;
    }
    let headers_start = newline + 1;
    let headers_end = headers_start.checked_add(headers_len)?;
    let headers_json: std::borrow::Cow<'_, [u8]> = if headers_end <= chunk.len() {
        std::borrow::Cow::Borrowed(&chunk[headers_start..headers_end])
    } else {
        // Headers record larger than the probe buffer — read the rest.
        let mut rest = vec![0u8; headers_end - chunk.len()];
        file.read_exact(&mut rest).ok()?;
        let mut whole = chunk[headers_start..].to_vec();
        whole.extend_from_slice(&rest);
        std::borrow::Cow::Owned(whole)
    };
    serde_json::from_slice(&headers_json).ok()
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
    read_mirror_headers(&mut file)
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
    let contents = fs::read(pkg_mirror).ok()?;
    let newline = contents.iter().position(|&byte| byte == b'\n')?;
    let line = std::str::from_utf8(&contents[..newline]).ok()?;
    let (headers_len, index_len) = parse_mirror_magic(line)?;
    let headers_start = newline + 1;
    let index_start = headers_start.checked_add(headers_len)?;
    let fragment_base = index_start.checked_add(index_len)?;
    if fragment_base > contents.len() {
        return None;
    }
    let headers: MetaHeaders =
        serde_json::from_slice(&contents[headers_start..index_start]).ok()?;
    let index: MirrorIndex = serde_json::from_slice(&contents[index_start..fragment_base]).ok()?;

    // Rebase the relative spans and reject any that fall outside the
    // file — a truncated or hand-edited mirror reads as a miss rather
    // than handing out garbage fragments later.
    let file_size = contents.len() as u64;
    let buffer = Arc::new(contents);
    let mut spans = Vec::with_capacity(index.versions.len());
    for (version, offset, len) in index.versions {
        let absolute = (fragment_base as u64).checked_add(offset)?;
        if absolute.checked_add(u64::from(len))? > file_size {
            return None;
        }
        spans.push((version, absolute, len));
    }

    Some(Package {
        name: index.name,
        dist_tags: index.dist_tags,
        versions: PackageVersions::from_buffer_spans(&buffer, spans),
        time: index.time,
        modified: headers.modified,
        etag: headers.etag,
        homepage: index.homepage,
        mutex: Arc::default(),
    })
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
pub fn save_meta(pkg_mirror: &Path, contents: &[u8]) -> Result<(), SaveMetaError> {
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
        file.write_all(contents)
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

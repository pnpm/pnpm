//! On-disk packument-mirror helpers.
//!
//! The cache-path and IO helpers the verifier needs to share the
//! resolver's metadata mirror:
//!
//! - [`get_pkg_mirror_path`] — `<cache_dir>/<meta_dir>/<registry-encoded>/<encoded-name>.jsonl`.
//! - [`load_meta_headers`] — read just the headers record (etag,
//!   modified) to feed conditional GETs without touching the rest.
//! - [`load_meta`] — read the headers + index records and reconstruct
//!   a [`Package`] whose versions hydrate from byte spans on demand.
//! - [`save_meta_indexed`] / [`save_meta_ndjson`] — atomic write via
//!   temp + rename so a torn write never leaks a half-formed mirror to
//!   the next install.
//!
//! ## File layout
//!
//! Pacquet's indexed format:
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
//! body.
//!
//! pnpm's two-line NDJSON format is also readable and is used when
//! writing filtered full metadata.
//!
//! Plus the constants and name-encoding rules:
//!
//! - [`FULL_META_DIR`] / [`FULL_FILTERED_META_DIR`] /
//!   [`ABBREVIATED_META_DIR`] — directory slugs pnpm and pacquet share.
//! - [`encode_pkg_name`] — mixed-case package names get a sha256 hex
//!   suffix so case-insensitive filesystems (HFS+, NTFS by default)
//!   can't collide two distinct package names onto one mirror file.
//! - [`get_registry_name`] — `host[:port]` with `:` → `+` (a
//!   filesystem-safe encoding).

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
use pnpm_network::MetadataCacheScope;
use pnpm_registry::{DerivedPackuments, MirrorFile, Package, PackageVersions};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Mirror directory for the **abbreviated** metadata cache.
pub const ABBREVIATED_META_DIR: &str = "v11/metadata";

/// Mirror directory for the **full** metadata cache.
pub const FULL_META_DIR: &str = "v11/metadata-full";

/// Mirror directory for the filtered full metadata cache.
pub const FULL_FILTERED_META_DIR: &str = "v11/metadata-full-filtered";

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
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_CREATE_DIR))]
    CreateDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to write mirror temp file {temp:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_WRITE_TEMP))]
    WriteTemp {
        temp: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Encode(#[error(source)] EncodeMetaError),
    #[display("Failed to rename mirror temp {temp:?} → {target:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_RENAME))]
    Rename {
        temp: PathBuf,
        target: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// The declared record lengths in a mirror's magic line come from the
/// file itself, so a corrupted or hostile mirror must never drive an
/// arbitrarily large allocation. The headers record is ~100 bytes of
/// etag + timestamp; the index scales with the version count (~100
/// bytes per version), so its bound leaves six-figure version counts
/// of headroom.
const MAX_HEADERS_LEN: usize = 64 * 1024;
const MAX_INDEX_LEN: usize = 64 * 1024 * 1024;

/// Ceiling for a single version fragment's declared span. The span
/// end is validated against the file size, but a sparse file makes
/// the file size itself untrustworthy — without a per-fragment bound
/// a corrupt mirror could declare a multi-gigabyte span and drive an
/// equally large hydration allocation.
const MAX_FRAGMENT_LEN: u32 = 16 * 1024 * 1024;

/// Mirror root for descriptor-scoped private metadata. A
/// [`MetadataCacheScope::Private`] route stores its packuments under
/// `<cache_dir>/v11/metadata-private/<descriptor-id>/<meta-suffix>/...`
/// so one caller's private metadata never lands in the global mirror
/// every other caller reads.
const PRIVATE_META_ROOT: &str = "v11/metadata-private";

/// The mirror directory `base_meta_dir` resolves to under `scope`.
///
/// * [`MetadataCacheScope::Public`] keeps the global directory unchanged
///   (the CLI and public routes).
/// * [`MetadataCacheScope::Private`] relocates it under
///   `v11/metadata-private/<descriptor-id>/` keyed by the descriptor id,
///   preserving the abbreviated/full/filtered split via the suffix after
///   `v11/`.
#[must_use]
pub fn scoped_meta_dir(scope: &MetadataCacheScope, base_meta_dir: &str) -> String {
    match scope {
        MetadataCacheScope::Public => base_meta_dir.to_string(),
        MetadataCacheScope::Private { descriptor_id } => {
            let suffix = base_meta_dir.strip_prefix("v11/").unwrap_or(base_meta_dir);
            format!("{PRIVATE_META_ROOT}/{descriptor_id}/{suffix}")
        }
    }
}

/// On-disk path of the JSONL document where pacquet mirrors a
/// package's registry metadata.
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
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_PARSE_REGISTRY))]
    ParseUrl {
        #[error(not(source))]
        url: String,
        error: String,
    },
    #[display("Registry URL {url:?} has no host")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_MISSING_HOST))]
    MissingHost {
        #[error(not(source))]
        url: String,
    },
}

/// `host[:port]` form of a registry URL with `:` rewritten to `+` so
/// the result is filesystem-safe. Only an explicit port participates;
/// the implicit-default port stays out of the slug so a registry served
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

/// Filesystem-safe form of a package name. A mixed-case name gets a
/// sha256 hex suffix so case-insensitive filesystems (HFS+, NTFS by
/// default) can't collide it with a lowercase sibling.
#[must_use]
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
#[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_MIRROR_ENCODE))]
pub struct EncodeMetaError(#[error(source)] serde_json::Error);

impl EncodeMetaError {
    pub(crate) fn into_inner(self) -> serde_json::Error {
        self.0
    }
}

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
        modified: meta_modified(meta),
    })
    .map_err(|error| SaveMetaError::Encode(EncodeMetaError(error)))?;

    let mut fragment_bytes = Vec::new();
    let mut spans = Vec::with_capacity(meta.versions.len());
    for (version, json) in meta.versions.fragments() {
        let offset = fragment_bytes.len() as u64;
        let len = u32::try_from(json.len()).unwrap_or(u32::MAX);
        if len as usize != json.len() || len > MAX_FRAGMENT_LEN {
            // A version manifest past the loader's fragment bound
            // would be persisted only to be skipped on every read;
            // omit it so the saved and served views agree.
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

/// Atomically persist `meta` at `pkg_mirror` in pnpm's two-line
/// NDJSON mirror format.
pub fn save_meta_ndjson(
    pkg_mirror: &Path,
    meta: &Package,
    etag: Option<&str>,
) -> Result<(), SaveMetaError> {
    let headers = serde_json::to_vec(&MetaHeaders {
        etag: etag.map(str::to_string),
        modified: meta_modified(meta),
    })
    .map_err(|error| SaveMetaError::Encode(EncodeMetaError(error)))?;
    let mut body_meta = meta.clone();
    body_meta.etag = None;
    let body = serde_json::to_vec(&body_meta)
        .map_err(|error| SaveMetaError::Encode(EncodeMetaError(error)))?;

    let mut bytes = Vec::with_capacity(headers.len() + 1 + body.len());
    bytes.extend_from_slice(&headers);
    bytes.push(b'\n');
    bytes.extend_from_slice(&body);
    save_meta(pkg_mirror, &bytes)
}

/// Strip full packuments down to the fields pnpm keeps when
/// `filterMetadata` is enabled.
pub fn clear_meta(meta: &Package) -> Result<Package, EncodeMetaError> {
    const VERSION_KEYS: &[&str] = &[
        "name",
        "version",
        "bin",
        "directories",
        "devDependencies",
        "optionalDependencies",
        "dependencies",
        "peerDependencies",
        "dist",
        "engines",
        "peerDependenciesMeta",
        "cpu",
        "os",
        "libc",
        "deprecated",
        "bundleDependencies",
        "bundledDependencies",
        "hasInstallScript",
        "_npmUser",
    ];

    let mut versions = Map::new();
    for (version, json) in meta.versions.fragments() {
        let info: Value = serde_json::from_str(&json).map_err(EncodeMetaError)?;
        let Value::Object(info) = info else {
            continue;
        };
        let mut filtered = Map::new();
        for key in VERSION_KEYS {
            if let Some(value) = info.get(*key) {
                filtered.insert((*key).to_string(), value.clone());
            }
        }
        versions.insert(version.clone(), Value::Object(filtered));
    }

    let mut pkg = Map::new();
    pkg.insert("name".to_string(), Value::String(meta.name.clone()));
    pkg.insert(
        "dist-tags".to_string(),
        serde_json::to_value(&meta.dist_tags).map_err(EncodeMetaError)?,
    );
    pkg.insert("versions".to_string(), Value::Object(versions));
    if let Some(time) = meta.time.as_ref() {
        pkg.insert("time".to_string(), serde_json::to_value(time).map_err(EncodeMetaError)?);
    }
    if let Some(modified) = meta.modified.as_ref() {
        pkg.insert("modified".to_string(), Value::String(modified.clone()));
    }

    let mut cleared: Package =
        serde_json::from_value(Value::Object(pkg)).map_err(EncodeMetaError)?;
    cleared.etag.clone_from(&meta.etag);
    Ok(cleared)
}

fn meta_modified(meta: &Package) -> Option<String> {
    meta.modified.clone().or_else(|| {
        meta.time
            .as_ref()
            .and_then(|time| time.get("modified"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

/// One-time, best-effort raise of the process's soft `RLIMIT_NOFILE`
/// toward the hard limit. Loaded mirrors keep their file handle open
/// so version fragments can be read on demand without buffering the
/// body (see [`load_meta`]), which holds one descriptor per packument
/// — beyond the conservative soft defaults some platforms ship (256
/// on macOS, 1024 on several Linux distros) once a workspace consults
/// thousands of packuments. Raising the soft limit to the hard limit
/// needs no privileges; it is the same startup adjustment the Go
/// runtime performs.
#[cfg(unix)]
fn raise_open_file_limit_once() {
    static RAISE: std::sync::Once = std::sync::Once::new();
    RAISE.call_once(|| {
        // SAFETY: plain libc calls; `limit` is a properly initialised
        // out-parameter and no pointer outlives its call.
        unsafe {
            let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) != 0 {
                return;
            }
            let ceiling: libc::rlim_t = 1 << 20;
            let target = limit.rlim_max.min(ceiling);
            if target <= limit.rlim_cur {
                return;
            }
            let request = libc::rlimit { rlim_cur: target, rlim_max: limit.rlim_max };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &raw const request) != 0 {
                // macOS rejects soft limits above `kern.maxfilesperproc`
                // even when the hard limit reads unlimited; 10240 is
                // the historically safe `OPEN_MAX` ceiling there.
                #[cfg(target_os = "macos")]
                {
                    let fallback = limit.rlim_max.min(10240);
                    if fallback > limit.rlim_cur {
                        let request = libc::rlimit { rlim_cur: fallback, rlim_max: limit.rlim_max };
                        let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &raw const request);
                    }
                }
            }
        }
    });
}

/// Windows has no `RLIMIT_NOFILE`; per-process handle capacity is far
/// above any realistic packument count.
#[cfg(not(unix))]
fn raise_open_file_limit_once() {}

/// Parse the `pacquet-meta-v1 <headers_len> <index_len>` line.
/// `None` for anything else, including pnpm's NDJSON format.
fn parse_mirror_magic(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix(MIRROR_MAGIC)?.strip_prefix(' ')?;
    let (headers_len, index_len) = rest.split_once(' ')?;
    Some((headers_len.parse().ok()?, index_len.parse().ok()?))
}

/// Read the headers record without touching the full metadata body.
/// `None` for anything unreadable.
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
    let Some((headers_len, _)) = parse_mirror_magic(line) else {
        return serde_json::from_str(line).ok();
    };
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
/// headers.
#[must_use]
pub fn load_meta_headers(pkg_mirror: &Path) -> Option<MetaHeaders> {
    let mut file = File::open(pkg_mirror).ok()?;
    read_mirror_headers(&mut file)
}

/// Read a mirror file's headers + index and reconstruct a [`Package`]
/// with its etag back-filled from the headers line.
///
/// For the indexed format only the header and index records are read
/// into memory; version fragments stay on disk behind the held-open
/// file handle ([`PackageVersions::from_file_spans`]), so a cache full
/// of multi-megabyte packuments costs their index size in resident
/// memory, not their body size. Past the held-handle budget (sized
/// from the descriptor limit) a load buffers its fragments instead of
/// keeping the file open. The legacy NDJSON format still parses the
/// whole body.
///
/// Returns `None` on missing file / malformed contents: the caller's
/// response to "couldn't read" is the same as "no cache".
#[must_use]
pub fn load_meta(pkg_mirror: &Path) -> Option<Package> {
    load_meta_with_hold_cap(pkg_mirror, held_mirror_file_cap())
}

fn load_meta_with_hold_cap(pkg_mirror: &Path, hold_cap: usize) -> Option<Package> {
    raise_open_file_limit_once();
    let mut file = File::open(pkg_mirror).ok()?;
    // The magic line plus the two length fields fit well inside this.
    let mut prefix = [0u8; 256];
    let mut filled = 0usize;
    while filled < prefix.len() {
        let read = file.read(&mut prefix[filled..]).ok()?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    let prefix = &prefix[..filled];
    let newline = prefix.iter().position(|&byte| byte == b'\n')?;
    let line = std::str::from_utf8(&prefix[..newline]).ok()?;
    let Some((headers_len, index_len)) = parse_mirror_magic(line) else {
        // Legacy NDJSON mirror — the whole body is the packument.
        let contents = fs::read(pkg_mirror).ok()?;
        let newline = contents.iter().position(|&byte| byte == b'\n')?;
        let headers: MetaHeaders = serde_json::from_slice(&contents[..newline]).ok()?;
        let mut meta: Package = serde_json::from_slice(&contents[newline + 1..]).ok()?;
        meta.etag = headers.etag;
        meta.modified = meta.modified.or(headers.modified);
        meta.drop_incomplete_publish_times();
        return Some(meta);
    };
    // Bound each declared length, then require the whole header +
    // index region to fit inside the actual file before allocating a
    // buffer for it.
    if headers_len > MAX_HEADERS_LEN || index_len > MAX_INDEX_LEN {
        return None;
    }
    let headers_start = newline + 1;
    let index_start = headers_start.checked_add(headers_len)?;
    let fragment_base = index_start.checked_add(index_len)?;
    let file_size = file.metadata().ok()?.len();
    if u64::try_from(fragment_base).ok()? > file_size {
        return None;
    }
    // Read the rest of the headers + index records; the file's
    // fragment section is only buffered on the held-handle fallback
    // below.
    let mut records = vec![0u8; fragment_base.checked_sub(filled.min(fragment_base))?];
    file.read_exact(&mut records).ok()?;
    let mut prefixed = Vec::with_capacity(fragment_base);
    prefixed.extend_from_slice(&prefix[..filled.min(fragment_base)]);
    prefixed.extend_from_slice(&records);
    let headers: MetaHeaders =
        serde_json::from_slice(prefixed.get(headers_start..index_start)?).ok()?;
    let index: MirrorIndex =
        serde_json::from_slice(prefixed.get(index_start..fragment_base)?).ok()?;

    // Rebase the relative spans and reject any that fall outside the
    // file — a truncated or hand-edited mirror reads as a miss rather
    // than handing out garbage fragments later.
    let mut spans = Vec::with_capacity(index.versions.len());
    for (version, offset, len) in index.versions {
        // A span past the fragment bound reads as an absent version
        // (the same contract as an undecodable fragment) rather than
        // rejecting the whole document: the bound exists to stop a
        // corrupt index from driving huge hydration allocations, and
        // the writer never persists such fragments.
        if len > MAX_FRAGMENT_LEN {
            continue;
        }
        let absolute = (fragment_base as u64).checked_add(offset)?;
        if absolute.checked_add(u64::from(len))? > file_size {
            return None;
        }
        spans.push((version, absolute, len));
    }

    let versions = match MirrorFile::try_hold(file, hold_cap) {
        Ok(held) => PackageVersions::from_file_spans(&held, spans),
        // Held-handle budget exhausted (an unusually low descriptor
        // limit, or an install consulting more packuments than the
        // cap): buffer this mirror's fragments and close the file, so
        // a full cache can never make `File::open` fail elsewhere and
        // turn present mirrors into cache misses.
        Err(file) => {
            // Each validated span is read with its own positioned read
            // and the file closed afterwards: reading the contiguous
            // fragment region would let a corrupt index's sparse gaps
            // inflate the buffer far past the real fragment bytes. The
            // budget bounds the total even against an index whose spans
            // overlap or repeat.
            const MAX_EAGER_FRAGMENT_TOTAL: u64 = 1 << 30;
            let mut budget = MAX_EAGER_FRAGMENT_TOTAL;
            let mut raw_fragments = Vec::with_capacity(spans.len());
            for (version, absolute, len) in spans {
                budget = budget.checked_sub(u64::from(len))?;
                let mut bytes = vec![0u8; len as usize];
                if pnpm_registry::read_exact_at(&file, &mut bytes, absolute).is_err() {
                    continue;
                }
                let Ok(json) = String::from_utf8(bytes) else { continue };
                let Ok(raw) = serde_json::from_str::<Box<serde_json::value::RawValue>>(&json)
                else {
                    continue;
                };
                raw_fragments.push((version, raw));
            }
            PackageVersions::from_raw_fragments(raw_fragments)
        }
    };

    let mut meta = Package {
        name: index.name,
        dist_tags: index.dist_tags,
        versions,
        time: index.time,
        modified: headers.modified,
        etag: headers.etag,
        homepage: index.homepage,
        mutex: Arc::default(),
        derived: DerivedPackuments::default(),
    };
    meta.drop_incomplete_publish_times();
    Some(meta)
}

/// How many mirror files [`load_meta`] may keep open at once. Sized
/// from the post-raise soft descriptor limit with headroom for the
/// rest of the install (sockets, tarball extraction, store writes);
/// loads beyond the cap buffer their fragments instead of holding a
/// handle.
fn held_mirror_file_cap() -> usize {
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CAP.get_or_init(|| {
        raise_open_file_limit_once();
        soft_open_file_limit().map_or(1 << 19, |soft| (soft / 2).min(1 << 19))
    })
}

#[cfg(unix)]
fn soft_open_file_limit() -> Option<usize> {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: plain libc call; `limit` is a properly initialised
    // out-parameter and the pointer does not outlive the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 {
        return None;
    }
    usize::try_from(limit.rlim_cur).ok()
}

/// Windows has no `RLIMIT_NOFILE`; the per-process handle capacity is
/// far above the cap's upper clamp.
#[cfg(not(unix))]
fn soft_open_file_limit() -> Option<usize> {
    None
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
/// body parses on the blocking pool.
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
/// over the target.
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
/// [`save_meta`] calls writing to sibling temp paths under the same
/// mirror directory. Pid + counter (a `<pid>.<counter>` suffix) is
/// enough to keep concurrent writers from colliding.
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

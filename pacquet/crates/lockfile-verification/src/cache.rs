//! On-disk verification cache.
//!
//! Verbatim port of pnpm's
//! [`verifyLockfileResolutionsCache.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutionsCache.ts).
//!
//! Two-index JSONL log at `<cache_dir>/lockfile-verified.jsonl`:
//!
//! - **by content hash** — recognizes the same lockfile across paths
//!   (worktrees, CI checkouts that reset stat fields, lockfile
//!   copies).
//! - **by absolute path** — same-machine stat shortcut. When the
//!   cached entry's `(size, mtime_ns, inode)` matches the current
//!   stat, trust the cached hash and skip reading the lockfile
//!   altogether.
//!
//! Every active verifier must still agree that the cached policy
//! snapshot is trustworthy under what it currently demands — that's
//! what [`ResolutionVerifier::can_trust_past_check`] decides.
//!
//! All IO is synchronous. The cache is consulted once before the
//! verifier fan-out and recorded once after; the brief blocking
//! `read` / `stat` calls don't overlap with any other in-flight
//! install work.
//!
//! [`ResolutionVerifier::can_trust_past_check`]: pacquet_resolving_resolver_base::ResolutionVerifier::can_trust_past_check

use chrono::{SecondsFormat, Utc};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

/// File name of the cache, relative to `cache_dir`. Matches
/// upstream's `CACHE_FILE_NAME` so a pnpm-populated cache file is
/// readable from pacquet and vice versa.
pub const CACHE_FILE_NAME: &str = "lockfile-verified.jsonl";

/// Hard cap on records the cache file holds after compaction.
/// Matches upstream's `MAX_CACHE_ENTRIES`. A developer machine that
/// touches a thousand distinct `(path, content)` tuples is far past
/// steady state.
pub const MAX_CACHE_ENTRIES: usize = 1000;

/// Compaction trigger in bytes. Records cluster around a few hundred
/// bytes; a 1.5 KiB-per-entry budget translates to ~1.5 MB with
/// generous slack so we don't trigger a rewrite on every append once
/// the cap is crossed. Matches upstream's `COMPACT_TRIGGER_BYTES`.
pub const COMPACT_TRIGGER_BYTES: u64 = (MAX_CACHE_ENTRIES as u64) * 1024 * 3 / 2;

/// One verified lockfile snapshot persisted to the JSONL log. Wire
/// shape matches upstream's `CacheRecord` field-for-field so the two
/// stacks share a cache file (pacquet reads pnpm's records and vice
/// versa — even though the hash values are unlikely to collide, the
/// stat shortcut still hits across both).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheRecord {
    pub lockfile: CacheLockfile,
    /// ISO-8601 timestamp of when the verification ran.
    #[serde(rename = "verifiedAt")]
    pub verified_at: String,
    /// Merged policy snapshot that passed when the verification ran.
    /// Every active [`ResolutionVerifier`]'s `policy()` contribution
    /// merges into the same map; same-key conflicts go to the last
    /// verifier in the list (a config bug we don't try to reconcile).
    pub policy: serde_json::Map<String, JsonValue>,
}

/// Lockfile identity slot inside a [`CacheRecord`]. Stat fields are
/// stringified because they can exceed `Number.MAX_SAFE_INTEGER` on
/// large filesystems / older nodes; pacquet keeps the same wire shape
/// for cross-stack compat. `inode = "0"` on platforms where the
/// concept doesn't apply (Windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheLockfile {
    /// sha256-hex of the lockfile content. Primary index key.
    pub hash: String,
    /// Absolute path the cache last saw this content at. Secondary
    /// index key for the stat fast path.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Lockfile mtime in nanoseconds, stringified (JSON numbers lose
    /// ns precision).
    #[serde(rename = "mtimeNs")]
    pub mtime_ns: String,
    /// Filesystem inode, stringified. `"0"` on platforms without
    /// inodes (Windows).
    pub inode: String,
}

/// Result of a [`try_lockfile_verification_cache`] lookup. `hit ==
/// true` lets the caller skip the verifier fan-out; `precomputed`
/// carries the stat + hash that the lookup already computed so the
/// matching [`record_verification`] call can skip re-doing them on
/// the miss-then-record path.
#[derive(Debug, Default, Clone)]
pub struct CacheLookupResult {
    pub hit: bool,
    pub precomputed: CachePrecomputed,
}

/// Precomputed lockfile fingerprint values shared between
/// [`try_lockfile_verification_cache`] and [`record_verification`].
#[derive(Debug, Default, Clone)]
pub struct CachePrecomputed {
    pub stat: Option<LockfileStat>,
    pub hash: Option<String>,
}

/// Stat fields the cache compares to decide whether the file at a
/// previously-cached path is still the file we saw. Cross-machine
/// values are meaningless; a fresh CI checkout that resets mtime
/// falls through to the content-hash lookup, which is the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockfileStat {
    pub size: u64,
    pub mtime_ns: String,
    pub inode: String,
}

/// Try to confirm a cached verification covers the lockfile as it
/// currently sits on disk **and** the policies currently in effect.
/// Returns `hit: true` so the caller can skip the verifier fan-out;
/// `hit: false` means the caller should run the gate and persist the
/// result with [`record_verification`].
///
/// Lookup order mirrors upstream:
///
/// 1. **Stat shortcut** — same path + same stat → trust the cached
///    hash; skip reading the lockfile.
/// 2. **Content lookup** — hash the lockfile and look up by hash.
///    Catches worktrees, CI checkouts where stat fields got reset.
///    On hit, refresh the path/stat slot so the next install at this
///    path takes the stat shortcut above.
///
/// `hash_lockfile` is a lazy closure: it's invoked only when the
/// stat shortcut doesn't apply, so a warm-stat install never pays
/// the hash cost. The closure is `FnMut` so callers can wrap a
/// memoised hash that's reused between the lookup and a
/// downstream [`record_verification`] call.
pub fn try_lockfile_verification_cache(
    cache_dir: &Path,
    lockfile_path: &Path,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    mut hash_lockfile: impl FnMut() -> String,
) -> CacheLookupResult {
    let Ok(indexes) = read_cache(cache_dir) else {
        // A corrupt cache file should never block the install;
        // fall through to verification so the gate still runs.
        return CacheLookupResult::default();
    };

    let Some(stat) = stat_lockfile(lockfile_path) else {
        return CacheLookupResult::default();
    };

    let path_key = lockfile_path.to_string_lossy().to_string();

    // Stat shortcut: same path + same stat means the cached hash is
    // still correct without reading the file.
    if let Some(record) = indexes.by_path.get(&path_key)
        && stat_matches(&stat, &record.lockfile)
    {
        return CacheLookupResult {
            hit: every_verifier_trusts_cached_run(record, verifiers),
            precomputed: CachePrecomputed {
                stat: Some(stat),
                hash: Some(record.lockfile.hash.clone()),
            },
        };
    }

    let hash = hash_lockfile();
    let Some(record) = indexes.by_hash.get(&hash) else {
        return CacheLookupResult {
            hit: false,
            precomputed: CachePrecomputed { stat: Some(stat), hash: Some(hash) },
        };
    };
    if !every_verifier_trusts_cached_run(record, verifiers) {
        return CacheLookupResult {
            hit: false,
            precomputed: CachePrecomputed { stat: Some(stat), hash: Some(hash) },
        };
    }

    // Refresh the byPath slot so the next install at this path takes
    // the stat shortcut. Failure here is best-effort: even if the
    // append fails, the cache contract still holds (we just won't
    // get the speedup at the new path).
    let refreshed = CacheRecord {
        lockfile: CacheLockfile {
            hash: record.lockfile.hash.clone(),
            path: path_key,
            size: stat.size,
            mtime_ns: stat.mtime_ns.clone(),
            inode: stat.inode.clone(),
        },
        verified_at: record.verified_at.clone(),
        policy: record.policy.clone(),
    };
    let _ = append_record(cache_dir, &refreshed);

    CacheLookupResult {
        hit: true,
        precomputed: CachePrecomputed { stat: Some(stat), hash: Some(hash) },
    }
}

/// Persist a successful verification. Mirrors upstream's
/// [`recordVerification`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutionsCache.ts#L320-L349).
///
/// Reuses `precomputed.stat` and `precomputed.hash` from a prior
/// [`try_lockfile_verification_cache`] call so the miss-then-record
/// path doesn't re-stat / re-hash the lockfile. When either is
/// missing, the function falls back to computing it. A
/// stat-after-the-fact failure (file disappeared since verification
/// began) silently drops the record — the gate already passed, the
/// install proceeds without the speedup next time.
pub fn record_verification(
    cache_dir: &Path,
    lockfile_path: &Path,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    mut hash_lockfile: impl FnMut() -> String,
    precomputed: CachePrecomputed,
) {
    let Some(stat) = precomputed.stat.or_else(|| stat_lockfile(lockfile_path)) else { return };
    let hash = precomputed.hash.unwrap_or_else(&mut hash_lockfile);
    let record = CacheRecord {
        lockfile: CacheLockfile {
            hash,
            path: lockfile_path.to_string_lossy().to_string(),
            size: stat.size,
            mtime_ns: stat.mtime_ns,
            inode: stat.inode,
        },
        verified_at: now_rfc3339(),
        policy: merge_policies(verifiers),
    };
    if append_record(cache_dir, &record).is_err() {
        return;
    }
    maybe_compact_cache(cache_dir);
}

struct CacheIndexes {
    /// Latest record per content hash — primary lookup.
    by_hash: HashMap<String, CacheRecord>,
    /// Latest record per absolute path — same-machine stat fast path.
    by_path: HashMap<String, CacheRecord>,
}

/// Read the cache file, building both indexes in one pass. Records
/// are walked in file order so the last record for any key wins
/// — matches upstream's `for (const line of contents.split('\n'))`
/// reduce. Returns empty indexes on `NotFound`; propagates other
/// IO errors so the caller can downgrade them to "no cache".
fn read_cache(cache_dir: &Path) -> io::Result<CacheIndexes> {
    let cache_file_path = cache_dir.join(CACHE_FILE_NAME);
    let contents = match fs::read_to_string(&cache_file_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CacheIndexes { by_hash: HashMap::new(), by_path: HashMap::new() });
        }
        Err(error) => return Err(error),
    };
    let mut by_hash = HashMap::new();
    let mut by_path = HashMap::new();
    for line in contents.lines() {
        if line.is_empty() {
            continue;
        }
        let parsed: CacheRecord = match serde_json::from_str(line) {
            Ok(value) => value,
            // Skip malformed lines; the next clean append still works.
            Err(_) => continue,
        };
        if parsed.lockfile.hash.is_empty() || parsed.lockfile.path.is_empty() {
            continue;
        }
        by_hash.insert(parsed.lockfile.hash.clone(), parsed.clone());
        by_path.insert(parsed.lockfile.path.clone(), parsed);
    }
    Ok(CacheIndexes { by_hash, by_path })
}

fn stat_lockfile(lockfile_path: &Path) -> Option<LockfileStat> {
    let metadata = fs::metadata(lockfile_path).ok()?;
    let size = metadata.len();
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or_else(|| "0".to_string(), |duration| duration.as_nanos().to_string());
    let inode = inode_of(&metadata);
    Some(LockfileStat { size, mtime_ns, inode })
}

#[cfg(unix)]
fn inode_of(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    metadata.ino().to_string()
}

#[cfg(not(unix))]
fn inode_of(_metadata: &fs::Metadata) -> String {
    // Windows has no inode equivalent the cache wants to compare
    // against; matching upstream's behavior leaves the slot empty
    // ("0") so the stat shortcut still works on Unix and degrades
    // to content-hash on Windows.
    "0".to_string()
}

fn stat_matches(stat: &LockfileStat, lockfile: &CacheLockfile) -> bool {
    stat.size == lockfile.size && stat.mtime_ns == lockfile.mtime_ns && stat.inode == lockfile.inode
}

fn every_verifier_trusts_cached_run(
    record: &CacheRecord,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    verifiers.iter().all(|verifier| verifier.can_trust_past_check(&record.policy))
}

fn merge_policies(verifiers: &[Arc<dyn ResolutionVerifier>]) -> serde_json::Map<String, JsonValue> {
    // Later verifiers overwrite earlier ones on conflict — a
    // shared-field convention; mismatch is a config bug we don't
    // try to reconcile.
    let mut merged = serde_json::Map::new();
    for verifier in verifiers {
        for (key, value) in verifier.policy() {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Append a record to the cache file. Single-line writes are atomic
/// on POSIX and NTFS, so concurrent pnpm / pacquet processes can
/// write without coordination.
fn append_record(cache_dir: &Path, record: &CacheRecord) -> io::Result<()> {
    fs::create_dir_all(cache_dir)?;
    let line = format!("{}\n", serde_json::to_string(record).map_err(io::Error::other)?);
    let cache_file_path = cache_dir.join(CACHE_FILE_NAME);
    OpenOptions::new().create(true).append(true).open(&cache_file_path)?.write_all(line.as_bytes())
}

fn maybe_compact_cache(cache_dir: &Path) {
    let cache_file_path = cache_dir.join(CACHE_FILE_NAME);
    let size = match fs::metadata(&cache_file_path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => return,
    };
    if size <= COMPACT_TRIGGER_BYTES {
        return;
    }
    let Ok(contents) = fs::read_to_string(&cache_file_path) else { return };

    // Walk reverse so the newest record per (path, hash) wins, drop
    // older duplicates, then trim to MAX_CACHE_ENTRIES.
    let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reversed: Vec<String> = Vec::new();
    for line in lines.iter().rev() {
        let parsed: CacheRecord = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if parsed.lockfile.hash.is_empty() || parsed.lockfile.path.is_empty() {
            continue;
        }
        let tuple_key = format!("{}\x00{}", parsed.lockfile.path, parsed.lockfile.hash);
        if !seen.insert(tuple_key) {
            continue;
        }
        reversed.push((*line).to_string());
    }
    reversed.reverse();
    let start = reversed.len().saturating_sub(MAX_CACHE_ENTRIES);
    let kept = &reversed[start..];

    // Write to a sibling tempfile + rename so a concurrent install
    // can't observe a half-written file.
    let temp_path = compact_temp_path(&cache_file_path);
    let mut new_contents = String::with_capacity(size as usize);
    for line in kept {
        new_contents.push_str(line);
        new_contents.push('\n');
    }
    if fs::write(&temp_path, new_contents.as_bytes()).is_err() {
        return;
    }
    if fs::rename(&temp_path, &cache_file_path).is_err() {
        let _ = fs::remove_file(&temp_path);
    }
}

static COMPACT_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn compact_temp_path(target: &Path) -> PathBuf {
    let counter = COMPACT_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let suffix = format!(".{pid}.{counter}.tmp");
    let mut name = match target.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_string(),
        None => "lockfile-verified".to_string(),
    };
    name.push_str(&suffix);
    target.with_file_name(name)
}

#[cfg(test)]
mod tests;

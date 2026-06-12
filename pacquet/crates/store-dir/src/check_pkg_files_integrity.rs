//! Port of pnpm v11's
//! [`store/cafs/src/checkPkgFilesIntegrity.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/store/cafs/src/checkPkgFilesIntegrity.ts).
//!
//! The store index's `package_index` row lists the CAFS paths a package
//! expanded into. Before reusing the row the caller checks those files
//! are still on disk and still match the recorded digests. This module
//! implements that check — with a fast path that skips filesystem work
//! entirely when the caller opted out of integrity verification.
//!
//! Mirrors the upstream structure function-for-function so a future
//! cross-reference (or a pnpm-side change we need to match) stays
//! cheap.

use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreDir};
use dashmap::DashSet;
use sha2::{Digest, Sha512};
use std::{
    collections::HashMap,
    fs,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

/// Set of CAFS paths whose on-disk integrity has already been verified
/// during the current install. Mirrors pnpm's
/// [`verifiedFilesCache: Set<string>`](https://github.com/pnpm/pnpm/blob/main/store/cafs/src/checkPkgFilesIntegrity.ts):
/// the caller threads one cache through every
/// [`check_pkg_files_integrity`] invocation so a CAFS blob that has
/// already been verified by package A doesn't get stat'd / re-hashed
/// again by package B.
///
/// Concurrent: the install fans `check_pkg_files_integrity` calls out
/// across tokio's blocking pool, so the cache must tolerate parallel
/// readers and writers. `DashSet` gives us that without any external
/// locking. Race-window duplicate verifies are benign (the `verify_file`
/// path is idempotent) and rare in practice.
pub type VerifiedFilesCache = DashSet<PathBuf>;

/// Shared handle to a [`VerifiedFilesCache`] — what every install-scope
/// caller passes around. `Arc` so the same cache survives across the
/// lockfile-driven and registry-driven install loops without
/// per-call clones, and so the value lives long enough to outlive the
/// individual `tokio::task::spawn_blocking` closures the verifier
/// dispatches into.
pub type SharedVerifiedFilesCache = Arc<VerifiedFilesCache>;

/// `in-tarball filename` → `CAFS path`. Return value of the two verify
/// entry points below.
pub type FilesMap = HashMap<String, PathBuf>;

/// Result of a `PackageFilesIndex`-row verification pass.
///
/// Mirrors pnpm's `VerifyResult`. `passed` is `false` if any referenced
/// CAFS file is missing, its size disagrees with the index, or its
/// content hash fails to match — the caller treats that as "this store
/// entry is stale, fall through to a fresh fetch". `files_map` is
/// returned either way as a best-effort `in-tarball filename` → `CAFS
/// path` map; it may be partial or empty when a digest in the index
/// row couldn't be reconstructed into a CAFS path, so callers should
/// gate reuse on `passed` rather than on the map's size.
///
/// `side_effects_maps` is the optional cache-key → overlaid-FilesMap
/// table from a populated side-effects cache (typically seeded by
/// pnpm). Each value is the post-build files map for one cache key:
/// the base `files_map` with the entry's `added` overlay applied on
/// top of it and `deleted` entries dropped. Mirrors
/// [`PackageFilesResponse.sideEffectsMaps`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/create-cafs-store/src/index.ts#L83-L100)
/// — the importer looks up the entry by the dep-state cache key
/// (`<engine>` or `<engine>;deps=…;patch=…`, produced by
/// `pacquet-graph-hasher`'s `calc_dep_state`) to decide whether
/// the package is already built.
#[derive(Debug)]
pub struct VerifyResult {
    pub passed: bool,
    pub files_map: FilesMap,
    pub side_effects_maps: Option<HashMap<String, FilesMap>>,
}

/// Fast path used when `verify-store-integrity` is `false`.
///
/// Port of pnpm's
/// [`buildFileMapsFromIndex`](https://github.com/pnpm/pnpm/blob/1819226b51/store/cafs/src/checkPkgFilesIntegrity.ts).
/// No stat syscalls — the caller trusts the index, and any missing /
/// corrupt CAFS file surfaces lazily at import time (pnpm's `linkOrCopy`
/// equivalent).
pub fn build_file_maps_from_index(store_dir: &StoreDir, entry: PackageFilesIndex) -> VerifyResult {
    let PackageFilesIndex { files, side_effects, .. } = entry;
    let mut files_map = HashMap::with_capacity(files.len());
    let mut passed = true;
    // Consume `entry.files` so the owned `String` filenames move into
    // `files_map` without a per-file clone. On a realistic install the
    // previous borrow-then-clone cost one allocation per file on every
    // warm cache hit.
    for (filename, info) in files {
        let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
            // A malformed digest (non-hex / too short) makes this entry
            // unreconstructable. pnpm's `getFilePathByModeInCafs` doesn't
            // validate and would crash at import time, so a `None` here
            // is pacquet-specific guardrail. We'd rather silently drop
            // the row than panic, but a partial `files_map` would leave
            // the caller with a cache hit missing package files — the
            // caller would proceed to link and end up with a broken
            // install. Flipping `passed` to `false` sends the whole
            // entry back through the re-fetch path so the install stays
            // consistent.
            tracing::debug!(
                target: "pacquet::store_index",
                ?filename,
                digest = %info.digest,
                "malformed CAFS digest in store-index row; re-fetching",
            );
            passed = false;
            continue;
        };
        files_map.insert(filename, path);
    }
    let side_effects_maps = build_side_effects_maps(store_dir, side_effects, &files_map);
    VerifyResult { passed, files_map, side_effects_maps }
}

/// Careful path used when `verify-store-integrity` is `true` (pnpm's
/// default).
///
/// Port of pnpm's `checkPkgFilesIntegrity`. Per file:
///
/// 1. `fs::metadata` the on-disk path to get its mtime + size.
/// 2. If `mtime - checked_at > 100 ms`, the file has been touched since
///    we last verified it. Compare sizes: mismatch → delete and fail;
///    match → re-hash the contents and compare against the stored
///    digest, deleting on mismatch.
/// 3. If the mtime is within 100 ms of the stored `checked_at`, trust
///    the digest and skip the hash — matches pnpm's own comment: "we
///    assume nobody will manually remove a file in the store and create
///    a new one".
///
/// Missing on disk (`ENOENT`) fails the whole entry so the caller
/// re-fetches. Unlike the prior pacquet implementation this does *not*
/// reject non-regular-file dirents preemptively — the integrity hash
/// catches real corruption, and pnpm doesn't guard against it in this
/// function either.
pub fn check_pkg_files_integrity(
    store_dir: &StoreDir,
    entry: PackageFilesIndex,
    verified_files_cache: &VerifiedFilesCache,
) -> VerifyResult {
    // Destructure so the owned `files` HashMap and `algo` String can be
    // consumed below; moving beats the extra per-file `filename.clone()`
    // the old borrow-based signature forced on the hot path.
    let PackageFilesIndex { files, algo, side_effects, .. } = entry;
    let mut all_verified = true;
    let mut files_map = HashMap::with_capacity(files.len());
    // `verified_files_cache` is the install-scoped
    // [`VerifiedFilesCache`] — pnpm's `verifiedFilesCache: Set<string>`.
    // Threading it through every call dedups across packages, not just
    // within one entry: a CAFS blob seen by package A's verify pass
    // skips the stat / re-hash when package B references it later.
    //
    // Key the set by the resolved CAFS path, not by `info.digest`. The
    // path factors in `info.mode` (via `-exec` suffix for executables
    // in `cas_file_path_by_mode`), so the same content digest can
    // legitimately appear under two distinct on-disk paths when the
    // tarball ships it with different executable bits. Digest-only
    // dedup would skip verifying the second path and happily return
    // `passed: true` with a stale / missing blob still on disk.
    for (filename, info) in files {
        let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
            tracing::debug!(
                target: "pacquet::store_index",
                ?filename,
                digest = %info.digest,
                "malformed CAFS digest in store-index row; re-fetching",
            );
            all_verified = false;
            continue;
        };
        if !verified_files_cache.contains(&path) {
            if verify_file(&path, &filename, &info, &algo) {
                // One `PathBuf` clone per unique CAFS path we actually
                // verified; zero for dedup hits. Strictly better than
                // the per-filename clone the borrow-based version had.
                //
                // Concurrency note: another thread may verify the same
                // path between the `contains` check and our `insert`,
                // doing the stat twice. That's benign — `verify_file`
                // is idempotent and the cache converges to the same
                // state either way. Pnpm's worker_threads cache has
                // the same race-window for the same reason.
                verified_files_cache.insert(path.clone());
            } else {
                all_verified = false;
            }
        }
        files_map.insert(filename, path);
    }
    let side_effects_maps = build_side_effects_maps(store_dir, side_effects, &files_map);
    VerifyResult { passed: all_verified, files_map, side_effects_maps }
}

/// Materialize the per-cache-key overlaid `FilesMap`s from a
/// `PackageFilesIndex.side_effects` entry. Mirrors upstream's
/// [`applySideEffectsDiffWithMaps`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/create-cafs-store/src/index.ts#L103-L121):
/// the overlay is `added` plus base entries that aren't in `deleted`,
/// with `added` winning when both name the same filename. The
/// content of `added` entries is *not* re-verified here — pnpm
/// doesn't do that either; corruption in the side-effects layer
/// would surface at import time via `linkOrCopy` failing on a
/// missing CAS blob.
///
/// Returns `None` when the entry has no `side_effects` field (the
/// common case for pacquet-written rows today) so callers can
/// trivially distinguish "no cache configured" from "cache configured
/// but empty".
fn build_side_effects_maps(
    store_dir: &StoreDir,
    side_effects: Option<HashMap<String, SideEffectsDiff>>,
    base_files: &FilesMap,
) -> Option<HashMap<String, FilesMap>> {
    let raw = side_effects?;
    let mut out: HashMap<String, FilesMap> = HashMap::with_capacity(raw.len());
    'next_key: for (cache_key, diff) in raw {
        let SideEffectsDiff { added, deleted } = diff;
        let mut overlay: FilesMap = HashMap::with_capacity(base_files.len());
        if let Some(added) = added {
            for (filename, info) in added {
                let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
                    // Skip the entire `cache_key` entry rather than
                    // returning a partial overlay. A future importer
                    // that flips `is_built = true` on overlay
                    // presence would otherwise turn a malformed
                    // digest into a silent corruption: build skipped
                    // but a required artifact missing from disk.
                    // Dropping the whole entry sends the package back
                    // through the rebuild path, which is safe.
                    tracing::debug!(
                        target: "pacquet::store_index",
                        ?filename,
                        digest = %info.digest,
                        cache_key,
                        "malformed CAFS digest in side-effects `added` overlay; dropping this cache_key entry entirely so the importer falls back to rebuild",
                    );
                    continue 'next_key;
                };
                overlay.insert(filename, path);
            }
        }
        // Promote `deleted` to a `HashSet` once per cache key so
        // the `base_files` walk stays linear in `|base|` instead of
        // `O(|base| * |deleted|)`. Pnpm's TS side keeps `deleted`
        // as a `Set` for the same reason.
        let deleted_set: std::collections::HashSet<String> =
            deleted.unwrap_or_default().into_iter().collect();
        for (filename, path) in base_files {
            if !deleted_set.contains(filename) && !overlay.contains_key(filename) {
                overlay.insert(filename.clone(), path.clone());
            }
        }
        out.insert(cache_key, overlay);
    }
    Some(out)
}

/// Port of pnpm's `verifyFile`. `true` when the on-disk file is either
/// unmodified since the last verified check or modified but still
/// content-hashes to the stored digest.
///
/// `filename` is the in-tarball path the caller is trying to reuse; it
/// doesn't affect behaviour, only the `debug!` log when verification
/// fails, so operators can see *which* package file invalidated the
/// store-index row in the log.
///
/// **Trust boundary.** This verification is for corruption detection
/// in a trusted local store. It is not a tamper boundary for a store
/// writable by untrusted users or jobs.
///
/// **Locking discipline.** The fast path (`is_modified == false`, i.e.
/// the file's mtime is within 100 ms of the recorded `checked_at`)
/// runs lock-free — it never touches the file's bytes and never
/// considers a delete, so it cannot race with an in-flight writer.
/// The slow path (where verification could lead to a
/// `remove_stale_cafs_entry` call) acquires
/// [`pacquet_fs::cas_write_lock`] for `path` before re-stating the
/// file. This is the same per-path mutex
/// [`pacquet_fs::ensure_file`] holds across `O_CREAT|O_EXCL` +
/// `write_all`, so a concurrent writer's full sequence completes
/// before the verifier evaluates the file. Without this gate, the
/// verifier observes the writer's intermediate (partial) state,
/// `unlink`s the file out from under the writer's open fd, and the
/// install ends up with `cas_paths` referencing a path whose dirent
/// has been removed — which surfaces later as ENOENT in
/// `link_file`.
fn verify_file(path: &Path, filename: &str, info: &CafsFileInfo, algo: &str) -> bool {
    // Lock-free fast path. `check_file` is read-only and only touches
    // the file's metadata; no risk of clobbering a writer's state.
    let Some((is_modified, _)) = check_file(path, info.checked_at) else {
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            "CAFS file missing or unreadable; re-fetching",
        );
        return false;
    };
    if !is_modified {
        return true;
    }

    // Slow path: the file's mtime indicates a recent change. Acquire
    // the per-path lock and re-check so a concurrent writer's
    // `write_all` lands before we decide whether to delete. The
    // common case (unmodified file from a prior install) never gets
    // here — the lock cost only applies to files actually being
    // re-verified, which is rare.
    let lock = pacquet_fs::cas_write_lock(path);
    let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    // Re-stat under the lock. The writer (if any) has finished by
    // now, so the size + mtime reflect the committed state. A file
    // that vanished between the fast-path check and lock acquisition
    // (concurrent prune or a sibling verifier that beat us in)
    // surfaces as ENOENT here and we propagate the cache miss
    // without trying to delete a path that's already gone.
    let Some((is_modified, size)) = check_file(path, info.checked_at) else {
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            "CAFS file disappeared between fast-path stat and lock acquisition; re-fetching",
        );
        return false;
    };
    if !is_modified {
        // Writer completed and the result happens to match
        // `checked_at` (uncommon but possible if `checked_at` was
        // updated very recently). Trust the cache, no further work.
        return true;
    }
    if size != info.size {
        // Wrong size → content definitely changed. Remove so the next
        // caller fetches a clean copy. See `remove_stale_cafs_entry`
        // for why this has to cover dirs too.
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            expected_size = info.size,
            actual_size = size,
            "CAFS file size mismatch; scrubbing and re-fetching",
        );
        remove_stale_cafs_entry(path);
        return false;
    }
    let passed = verify_file_integrity(path, &info.digest, algo);
    if !passed {
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            "CAFS file digest mismatch or unknown algo; scrubbing and re-fetching",
        );
        remove_stale_cafs_entry(path);
    }
    passed
}

/// Remove a CAFS dirent that failed verification, matching pnpm's
/// `rimrafSync` semantics.
///
/// `fs::remove_file` on a directory returns `EISDIR` / `EPERM`, and a
/// corrupted store that has a directory sitting where a CAFS blob
/// belongs (stray `mkdir -p`, interrupted write, filesystem hiccup)
/// would stay there forever if we only tried `remove_file`. Next
/// install's verification would fail again and again — the store
/// wouldn't self-heal.
///
/// Best-effort for both: try `remove_file`, fall back to
/// `remove_dir_all` if the dirent is a directory. Errors are logged at
/// `debug` and dropped — worst case the next install notices the same
/// stale dirent and retries. We use `symlink_metadata` so we identify
/// the dirent type without following a symlink.
fn remove_stale_cafs_entry(path: &Path) {
    let is_dir = fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir());
    let result = if is_dir { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    if let Err(error) = result {
        tracing::debug!(
            target: "pacquet::store_index",
            ?path,
            ?error,
            "failed to scrub stale CAFS entry; next install will retry",
        );
    }
}

/// Port of pnpm's `checkFile`. `Some((is_modified, size))` for a file
/// we can read metadata for; `None` otherwise.
///
/// Pnpm rethrows non-`ENOENT` errors and only returns `null` for
/// `ENOENT`. This port collapses every metadata error (permission
/// denied, EIO, platform mtime representation failures) to `None`
/// instead, which the caller then treats as "verification failed →
/// re-fetch". That's a safer default for a cache-hint path — we don't
/// want a transient `EACCES` on a CAS blob to panic the install — and
/// the content-hash check in `verify_file_integrity` still catches
/// actual corruption. If we ever want pnpm-strict error propagation,
/// changing the return type to `Result<Option<…>>` is the right shape.
///
/// 100 ms of slack on the mtime comparison matches pnpm's threshold —
/// accounts for coarse mtime resolution on some filesystems plus the
/// ≤1 ms drift between when we recorded `checked_at` and when the kernel
/// actually stamped the inode. A missing `checked_at` deserializes as
/// `Option<u64>::None` and is treated as `0`, which forces a re-hash the
/// first time an old-format row is read (same as pnpm's `?? 0`).
fn check_file(path: &Path, checked_at: Option<u64>) -> Option<(bool, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let baseline = checked_at.unwrap_or(0);
    let is_modified = mtime_ms.saturating_sub(baseline) > 100;
    Some((is_modified, meta.len()))
}

/// Port of pnpm's `verifyFileIntegrity`. Streams the file through the
/// hasher in 64 KiB chunks and compares the digest against the stored
/// hex `digest`.
///
/// pnpm itself calls `readFileSync` + `crypto.hash`, which loads the
/// whole blob into a `Buffer` first. On Node that's capped implicitly
/// by `Buffer.kMaxLength`; in Rust we'd allocate the full file up
/// front, spiking RSS for multi-MB CAS blobs when an install is
/// verifying many entries in parallel. A `BufReader` + incremental
/// `Digest::update` is equivalent on the wire and keeps peak memory
/// bounded per thread.
///
/// Only `sha512` is supported — pacquet always writes that algo in
/// [`StoreDir::write_cas_file`]. Any other algo falls through to
/// `false` ("treat as verification failure"), matching pnpm's own
/// unknown-algo behaviour. An I/O error mid-read also falls through to
/// `false` so the caller re-fetches rather than deciding on a partial
/// hash.
fn verify_file_integrity(path: &Path, digest: &str, algo: &str) -> bool {
    if algo != "sha512" {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            // `Interrupted` is the one error we retry — it's a signal,
            // not a real IO failure. Everything else (NotFound, EIO,
            // PermissionDenied, ...) short-circuits to `false` so the
            // caller re-fetches.
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    format!("{:x}", hasher.finalize()) == digest
}

#[cfg(test)]
mod tests;

//! The store index's `package_index` row lists the CAFS paths a package
//! expanded into. Before reusing the row the caller checks those files
//! are still on disk and still match the recorded digests. This module
//! implements that check — with a fast path that skips filesystem work
//! entirely when the caller opted out of integrity verification.

use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreDir};
use dashmap::DashSet;
use sha2::{Digest, Sha512};
use std::{
    collections::HashMap,
    fs,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

/// Process-wide tally of the CAFS files [`verify_file_integrity`] had
/// to re-hash, and the time that hashing took. Content hashing is the
/// expensive half of store verification — the common case never reaches
/// it, because the recorded `checked_at` says the file is untouched. A
/// high tally means something keeps invalidating the store (a
/// mtime-rewriting backup tool, an antivirus scanner, a shared store on
/// a filesystem with coarse timestamps), so the install reports it.
///
/// Process-global rather than install-scoped: the verifiers run deep
/// inside the fetch/import fan-out while the report is emitted at the
/// end of the install, and every hop between the two is on the hot
/// path. Callers that want the figures for one install take a
/// [`VerifiedFileIntegrity`] snapshot when it starts and diff against
/// it — see [`VerifiedFileIntegrity::since`].
///
/// That diff is exact because the CLI installs one project at a time,
/// including the per-project loop dedicated lockfiles take. An embedder
/// driving several installs at once in one process is the one case
/// where a diff can pick up a sibling's hashing.
static VERIFIED_FILE_INTEGRITY: VerifiedFileIntegrityTally =
    VerifiedFileIntegrityTally { files: AtomicU64::new(0), nanos: AtomicU64::new(0) };

struct VerifiedFileIntegrityTally {
    files: AtomicU64,
    nanos: AtomicU64,
}

impl VerifiedFileIntegrityTally {
    /// The two counters move separately, so a reader running alongside
    /// a hashing thread can catch one of them mid-update. The install
    /// that reports the figures has already awaited its own
    /// verification, so it never races itself; only a second install
    /// hashing concurrently in the same process can, and its work
    /// perturbs the figures either way. Time goes in first regardless,
    /// so the duration the report gates on is never the half left
    /// behind.
    ///
    /// Saturating so a pathological duration can't wrap the tally into
    /// a small number and silence the report. 2^64 ns is ~584 years, so
    /// this never fires in practice.
    fn record(&self, elapsed: Duration) {
        self.nanos
            .fetch_add(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
        self.files.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> VerifiedFileIntegrity {
        VerifiedFileIntegrity {
            files: self.files.load(Ordering::Relaxed),
            duration: Duration::from_nanos(self.nanos.load(Ordering::Relaxed)),
        }
    }
}

/// How many CAFS files have been content-hashed, and how long that
/// hashing took.
///
/// Only the careful path ([`check_pkg_files_integrity`]) hashes, and
/// only for a file whose `mtime` moved past its recorded `checked_at` —
/// so a warm store on an install that nothing has disturbed leaves both
/// figures at zero. `duration` is summed across the threads doing the
/// hashing, so it is the work spent, which can exceed the install's
/// wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedFileIntegrity {
    pub files: u64,
    pub duration: Duration,
}

impl VerifiedFileIntegrity {
    /// What this process has hashed so far. Snapshot it when an install
    /// starts and [`Self::since`] the result at the end to get that
    /// install's own figures.
    #[must_use]
    pub fn snapshot() -> Self {
        VERIFIED_FILE_INTEGRITY.snapshot()
    }

    /// This snapshot minus an earlier one. Saturating, so a caller that
    /// diffs snapshots in the wrong order gets zeroes rather than an
    /// enormous bogus figure.
    #[must_use]
    pub fn since(self, baseline: Self) -> Self {
        VerifiedFileIntegrity {
            files: self.files.saturating_sub(baseline.files),
            duration: self.duration.saturating_sub(baseline.duration),
        }
    }
}

/// Set of CAFS paths whose on-disk integrity has already been verified
/// during the current install. The caller threads one cache through
/// every [`check_pkg_files_integrity`] invocation so a CAFS blob that
/// has already been verified by package A doesn't get stat'd /
/// re-hashed again by package B.
///
/// Concurrent: the install fans [`check_pkg_files_integrity`] calls out
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
/// When `passed` is `false` the caller treats the store entry as stale
/// and falls through to a fresh fetch. `files_map` is returned either
/// way as a best-effort `in-tarball filename` → `CAFS path` map; it may
/// be partial or empty, so callers should gate reuse on `passed` rather
/// than on the map's size.
///
/// `side_effects_maps` is the optional cache-key → overlaid-FilesMap
/// table from a populated side-effects cache (typically seeded by
/// pnpm). Each value is the post-build files map for one cache key:
/// the base `files_map` with the entry's `added` overlay applied on
/// top of it and `deleted` entries dropped. The importer looks up the
/// entry by the dep-state cache key (`<engine>` or
/// `<engine>;deps=…;patch=…`, produced by `pnpm-graph-hasher`'s
/// `calc_dep_state`) to decide whether the package is already built.
#[derive(Debug)]
pub struct VerifyResult {
    pub passed: bool,
    pub files_map: FilesMap,
    pub side_effects_maps: Option<HashMap<String, FilesMap>>,
    pub side_effects: Option<HashMap<String, SideEffectsDiff>>,
    pub remote_side_effects_quarantine: Option<HashMap<String, Vec<String>>>,
}

/// Fast path used when `verify-store-integrity` is `false`.
///
/// No stat syscalls — the caller trusts the index, and any missing /
/// corrupt CAFS file surfaces lazily at import time.
pub fn build_file_maps_from_index(store_dir: &StoreDir, entry: PackageFilesIndex) -> VerifyResult {
    let PackageFilesIndex { files, side_effects, remote_side_effects_quarantine, .. } = entry;
    let mut files_map = HashMap::with_capacity(files.len());
    let mut passed = true;
    // Consume `entry.files` so the owned `String` filenames move into
    // `files_map` without a per-file clone.
    for (filename, info) in files {
        let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
            // A malformed digest (non-hex / too short) makes this entry
            // unreconstructable. pnpm doesn't validate the digest and
            // would crash at import time; this `None` is a
            // pacquet-specific guardrail.
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
    let side_effects_maps = build_side_effects_maps(store_dir, side_effects.as_ref(), &files_map);
    VerifyResult {
        passed,
        files_map,
        side_effects_maps,
        side_effects,
        remote_side_effects_quarantine,
    }
}

/// Careful path used when `verify-store-integrity` is `true` (the
/// default).
///
/// Verifies every CAFS file the index row references and fails the
/// whole entry — so the caller re-fetches — when any file no longer
/// matches what the row recorded. Non-regular-file dirents are *not*
/// rejected preemptively — the integrity hash catches real corruption,
/// and pnpm doesn't guard against it preemptively either.
pub fn check_pkg_files_integrity(
    store_dir: &StoreDir,
    entry: PackageFilesIndex,
    verified_files_cache: &VerifiedFilesCache,
) -> VerifyResult {
    // Destructure so the owned `files` HashMap and `algo` String can be
    // consumed below, moving the filenames into `files_map` without a
    // per-file clone on the hot path.
    let PackageFilesIndex { files, algo, side_effects, remote_side_effects_quarantine, .. } = entry;
    let mut all_verified = true;
    let mut files_map = HashMap::with_capacity(files.len());
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
    let side_effects_maps = build_side_effects_maps(store_dir, side_effects.as_ref(), &files_map);
    VerifyResult {
        passed: all_verified,
        files_map,
        side_effects_maps,
        side_effects,
        remote_side_effects_quarantine,
    }
}

/// Materialize the per-cache-key overlaid [`FilesMap`]s from a
/// `PackageFilesIndex.side_effects` entry. The content of `added`
/// entries is *not* re-verified here — pnpm doesn't do that either;
/// corruption in the side-effects layer would surface at import time
/// as a missing CAS blob.
fn build_side_effects_maps(
    store_dir: &StoreDir,
    side_effects: Option<&HashMap<String, SideEffectsDiff>>,
    base_files: &FilesMap,
) -> Option<HashMap<String, FilesMap>> {
    let raw = side_effects?;
    let mut out: HashMap<String, FilesMap> = HashMap::with_capacity(raw.len());
    'next_key: for (cache_key, diff) in raw {
        let SideEffectsDiff { added, deleted, .. } = diff;
        let mut overlay: FilesMap = HashMap::with_capacity(base_files.len());
        if let Some(added) = added {
            for (filename, info) in added {
                // The overlay map is later joined onto the package
                // directory and written during import, so a poisoned /
                // corrupted index row (store integrity is explicitly not
                // a tamper boundary — see `verify_file`) could otherwise
                // escape the slot via a `..` or absolute `added` key.
                if !is_safe_overlay_path(filename) {
                    tracing::debug!(
                        target: "pacquet::store_index",
                        ?filename,
                        cache_key,
                        "unsafe path in side-effects `added` overlay; dropping this cache_key entry entirely so the importer falls back to rebuild",
                    );
                    continue 'next_key;
                }
                let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
                    // A future importer that flips `is_built = true` on
                    // overlay presence would otherwise turn a malformed
                    // digest into a silent corruption: build skipped but
                    // a required artifact missing from disk.
                    tracing::debug!(
                        target: "pacquet::store_index",
                        ?filename,
                        digest = %info.digest,
                        cache_key,
                        "malformed CAFS digest in side-effects `added` overlay; dropping this cache_key entry entirely so the importer falls back to rebuild",
                    );
                    continue 'next_key;
                };
                overlay.insert(filename.clone(), path);
            }
        }
        // Promote `deleted` to a `HashSet` once per cache key so
        // the `base_files` walk stays linear in `|base|` instead of
        // `O(|base| * |deleted|)`.
        let deleted_set: std::collections::HashSet<String> =
            deleted.iter().flatten().cloned().collect();
        for (filename, path) in base_files {
            if !deleted_set.contains(filename) && !overlay.contains_key(filename) {
                overlay.insert(filename.clone(), path.clone());
            }
        }
        out.insert(cache_key.clone(), overlay);
    }
    Some(out)
}

/// Whether `filename` is a safe package-relative path to write under the
/// package slot. Rejects absolute paths, any `..` component, and `\`
/// separators (which are `Normal` components on Unix but path separators
/// on Windows). Mirrors the path-traversal guard the tarball extractor
/// applies to archive entries — the side-effects overlay reaches the same
/// `dir.join(key)` import, so the same rule applies to a store row that
/// can't be trusted not to have been tampered with.
fn is_safe_overlay_path(filename: &str) -> bool {
    use std::path::Component;
    if filename.is_empty() || filename.contains('\\') {
        return false;
    }
    let path = Path::new(filename);
    path.components().all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// `true` when the on-disk file is either unmodified since the last
/// verified check or modified but still content-hashes to the stored
/// digest.
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
/// runs lock-free — it never touches the file's bytes, so it cannot
/// race with an in-flight writer. The slow path acquires
/// [`pnpm_fs::cas_write_lock`] for `path` before re-stating the
/// file. This is the same per-path mutex
/// [`pnpm_fs::ensure_file`] holds across `O_CREAT|O_EXCL` +
/// `write_all`, so a concurrent same-process writer's full sequence
/// completes before the verifier evaluates the file — without the
/// gate the verifier would read the writer's intermediate (partial)
/// state and report a spurious miss. The lock is process-local, which
/// is exactly why a failed verification must never unlink a file:
/// another *process* sharing the store may be importing from it — see
/// [`scrub_directory_at_cafs_path`].
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
    let lock = pnpm_fs::cas_write_lock(path);
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
        // Wrong size → content definitely changed. Report the miss so
        // the caller re-fetches; see `scrub_directory_at_cafs_path` for
        // why nothing but a directory is removed here.
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            expected_size = info.size,
            actual_size = size,
            "CAFS file size mismatch; re-fetching",
        );
        scrub_directory_at_cafs_path(path);
        return false;
    }
    let passed = verify_file_integrity(path, &info.digest, algo);
    if !passed {
        tracing::debug!(
            target: "pacquet::store_index",
            ?filename,
            ?path,
            "CAFS file digest mismatch, read failure, or unknown algo; re-fetching",
        );
        scrub_directory_at_cafs_path(path);
    }
    passed
}

/// Remove a *directory* squatting at a CAFS blob path so the
/// re-fetch's `rename` can land. Every other dirent stays: the
/// re-fetch replaces a mismatched file atomically, and unlinking here
/// would race installs in other processes importing from this very
/// path (pnpm/pnpm#14353's error class) — the verifier's lock is
/// process-local, and [`verify_file_integrity`] reports a transient
/// read failure the same way as a real mismatch.
///
/// A check-then-delete on the live path could delete whatever a
/// concurrent process put there in between, so the dirent is renamed
/// aside first and only then inspected: a directory is removed at its
/// scrub name, and anything else — including a blob a concurrent
/// re-fetch landed after the failed verification — is renamed back
/// where it was. `remove_dir_all` is never pointed at the live path
/// (it would unlink a terminal symlink there, not just a directory).
///
/// Best-effort throughout: a failure is logged at `debug` and the next
/// install retries. A crash between the two renames leaves an inert
/// `*.pacquet-scrub-*` entry in the shard directory, which nothing
/// ever resolves.
fn scrub_directory_at_cafs_path(path: &Path) {
    // Cheap gate only — the rename + inspect below re-decides
    // authoritatively, so a dirent swapped after this stat is still
    // handled correctly. Without the gate every mismatched *file*
    // would pay the rename round-trip and briefly vanish from its
    // path.
    if !fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir()) {
        return;
    }
    static SCRUB_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut scrub_path = path.as_os_str().to_owned();
    scrub_path.push(format!(
        ".pacquet-scrub-{}-{}",
        std::process::id(),
        SCRUB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let scrub_path = Path::new(&scrub_path);
    if fs::rename(path, scrub_path).is_err() {
        // Nothing left at the path (a concurrent scrubber won), or the
        // rename is not possible; either way the next install retries.
        return;
    }
    let is_dir = fs::symlink_metadata(scrub_path).is_ok_and(|meta| meta.file_type().is_dir());
    let result = if is_dir {
        fs::remove_dir_all(scrub_path)
    } else {
        // The dirent changed between the failed verification and the
        // rename — a concurrent process replaced the squatter. Put the
        // newcomer back where every reader expects it.
        fs::rename(scrub_path, path)
    };
    if let Err(error) = result {
        tracing::debug!(
            target: "pacquet::store_index",
            ?path,
            ?scrub_path,
            ?error,
            "failed to scrub a directory at a CAFS path; next install will retry",
        );
    }
}

/// `Some((is_modified, size))` for a file we can read metadata for;
/// `None` otherwise.
///
/// Pnpm rethrows non-`ENOENT` metadata errors and only treats `ENOENT`
/// as a miss. Pacquet instead collapses every metadata error
/// (permission denied, EIO, platform mtime representation failures) to
/// `None`, which the caller then treats as "verification failed →
/// re-fetch". That's a safer default for a cache-hint path — we don't
/// want a transient `EACCES` on a CAS blob to panic the install — and
/// the content-hash check in [`verify_file_integrity`] still catches
/// actual corruption. If we ever want pnpm-strict error propagation,
/// changing the return type to `Result<Option<…>>` is the right shape.
///
/// 100 ms of slack on the mtime comparison matches pnpm's threshold —
/// accounts for coarse mtime resolution on some filesystems plus the
/// ≤1 ms drift between when we recorded `checked_at` and when the kernel
/// actually stamped the inode. A missing `checked_at` deserializes as
/// `Option<u64>::None` and is treated as `0`, which forces a re-hash the
/// first time an old-format row is read (as pnpm also does).
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

/// Whether the materialized package under `dir` still matches the store
/// row it was expanded from.
///
/// This is pnpm's `dint.check`, and answers what `pnpm store status`
/// asks: has anything edited the package after it was linked out of the
/// store. Only the files the row records are checked — a file *added*
/// under `dir` afterwards is not a change to what the store holds — and
/// a missing, unreadable, or re-hashed file all read as mutated.
#[must_use]
pub fn package_dir_matches_index(dir: &Path, index: &PackageFilesIndex) -> bool {
    index.files.iter().all(|(path, file)| {
        join_inside(dir, path)
            .is_some_and(|path| verify_file_integrity(&path, &file.digest, &index.algo))
    })
}

/// `dir` joined with a recorded in-package path, or `None` if that path is
/// anything other than a sequence of plain names.
///
/// The recorded paths come from archive entries. Extraction rejects a
/// leading separator and `..`, but not a Windows drive prefix — and
/// [`Path::join`] discards its base when the argument has one, which would
/// point the hash at a file outside the package. Rejecting here keeps that
/// decision local to the one caller that joins index keys onto a
/// directory rather than reading them out of the CAS.
fn join_inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let mut joined = dir.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(segment) => joined.push(segment),
            _ => return None,
        }
    }
    Some(joined)
}

/// Streams the file through the hasher in 64 KiB chunks and compares
/// the digest against the stored hex `digest`.
///
/// Reading the whole blob into memory before hashing would allocate
/// the full file up front, spiking RSS for multi-MB CAS blobs when an
/// install is verifying many entries in parallel. A `BufReader` +
/// incremental `Digest::update` produces the same digest and keeps
/// peak memory bounded per thread.
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
    // Timed from here, where the bytes actually start moving: the two
    // guards above cost nothing worth attributing to the store, and a
    // count that included them would inflate on a corrupt index rather
    // than on slow hashing.
    let started = Instant::now();
    let matches = hash_matches(file, digest);
    VERIFIED_FILE_INTEGRITY.record(started.elapsed());
    matches
}

/// Streams `file` through the hasher and compares the result against
/// the stored hex `digest`.
fn hash_matches(file: fs::File, digest: &str) -> bool {
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

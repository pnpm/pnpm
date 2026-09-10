use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use sha2::Digest as _;
use std::{
    collections::HashMap,
    io::{Read as _, Write},
    path::{Path, PathBuf},
};

/// Bumped whenever the derived suffixes or this file's encoding
/// change, so entries written by an older pnpm are never read.
pub(super) const CACHE_FORMAT_VERSION: &str = "1";

/// Generous per-snapshot ceiling on a cache file, so a preseeded one
/// cannot make an install read an arbitrary amount before it has
/// validated anything. A real entry is a package key plus a slot
/// suffix and two length prefixes; both strings are bounded in
/// practice by npm's 214-character name limit plus a version and a
/// 64-character digest.
const MAX_ENTRY_BYTES: u64 = 4096;

/// One file per lockfile directory, not one per fingerprint.
///
/// The fingerprint changes with every lockfile edit, engine change
/// and build-policy change, so filing entries under it would leave
/// a snapshot-sized file behind in the user's cache for each — and
/// nothing prunes them. Naming the file after the project instead
/// and keeping the fingerprint *inside* it means the newest entry
/// replaces the previous one, at the cost of not being able to
/// switch between two lockfiles without re-deriving.
fn cache_path(cache_dir: &Path, lockfile_dir: &Path) -> PathBuf {
    let mut hasher = sha2::Sha256::new();
    hasher.update(CACHE_FORMAT_VERSION.as_bytes());
    hasher.update(lockfile_dir.to_string_lossy().as_bytes());
    cache_dir.join("gvs-layout").join(format!("{:x}.bin", hasher.finalize()))
}

/// Where one project's entry lives and what it must have been
/// derived from.
#[derive(Clone, Copy)]
pub(super) struct CacheFile<'a> {
    pub cache_dir: &'a Path,
    pub lockfile_dir: &'a Path,
    pub fingerprint: &'a str,
}

/// Read a cached map back, or `None` to derive it instead.
///
/// Length-prefixed pairs — `u32 key_len | key | u32 val_len | value`
/// — rather than JSON: the map runs to thousands of entries and the
/// whole point is to beat the derivation it replaces.
///
/// What comes back off disk is checked against `expected` before it
/// is trusted, because a suffix decides where a package is linked
/// from. Nothing here re-derives a hash — that is the work being
/// avoided — but two things are cheap and rule out the ways a wrong
/// map does damage:
///
///   * every snapshot the caller holds must be present. A file
///     truncated between pairs otherwise parses as a shorter map,
///     and each absent snapshot silently takes
///     [`super::VirtualStoreLayout::slot_dir`]'s flat-name fallback,
///     landing outside the global virtual store;
///   * every suffix must name the package it is filed under, and
///     end in something shaped like a graph digest.
///     `cacheDir` is settable from a repository's own
///     `pnpm-workspace.yaml`, so a hostile repository can commit a
///     cache entry; the first check stops one from pointing a
///     package at a slot holding some *other* package, and
///     [`is_graph_node_digest`] stops one from pointing it out of
///     the store entirely.
///
/// Neither check says the digest is *this* snapshot's, because
/// establishing that means computing it, which is the work being
/// skipped. An entry can therefore still name another slot of the
/// same `name@version` — one some other project in the shared store
/// derived, carrying that project's child links rather than this
/// lockfile's. What stops the install from adopting it is
/// downstream: `slot_contents_complete` probes every child link the
/// snapshot's own dependencies imply before treating a slot as
/// materialized, so a foreign variant is rewritten rather than
/// reused. The cost of a suffix that lies is a slot written at the
/// wrong path, not a package linked against the wrong dependencies.
pub(super) fn load(
    file: CacheFile<'_>,
    expected: Expected<'_>,
) -> Option<HashMap<PackageKey, String>> {
    // A hostile checkout can choose `cacheDir` and so write this
    // file, and it has to be read before any of it can be checked.
    // The read is therefore bounded by what a legitimate map for
    // *these* snapshots could need. Nothing observable changes when
    // the bound is hit — an over-long file fails validation below
    // either way — so this only caps the memory a preseeded one can
    // make an install allocate.
    let path = cache_path(file.cache_dir, file.lockfile_dir);
    // A hostile checkout can point `cacheDir` at a directory it
    // ships, so this path may be anything it likes. Opening a FIFO
    // blocks until someone writes to it, which would hang the
    // install before it has read a single package: require a
    // regular file, and one that is not reached through a symlink,
    // before opening.
    if !std::fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let handle = std::fs::File::open(&path).ok()?;
    if !handle.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    let ceiling = (expected.snapshots.len() as u64 + 1).saturating_mul(MAX_ENTRY_BYTES);
    handle.take(ceiling).read_to_end(&mut bytes).ok()?;
    let (stored_fingerprint, mut cursor) = read_field(&bytes, 0)?;
    if stored_fingerprint != file.fingerprint {
        return None;
    }
    let mut suffixes = HashMap::with_capacity(expected.snapshots.len());
    while cursor < bytes.len() {
        let (package_key, next) = read_field(&bytes, cursor)?;
        let (suffix, next) = read_field(&bytes, next)?;
        cursor = next;
        let package_key = package_key.parse::<PackageKey>().ok()?;
        let digest = suffix.strip_prefix(&expected.slot_prefix(&package_key)?)?;
        if !is_graph_node_digest(digest) {
            return None;
        }
        suffixes.insert(package_key, suffix.to_owned());
    }
    expected.snapshots.keys().all(|key| suffixes.contains_key(key)).then_some(suffixes)
}

/// Whether `digest` is the hex `calc_graph_node_hash` produces, and
/// so a single path component.
///
/// The prefix check above says a suffix names the right package;
/// this says the rest of it is a name and not a route. Without it a
/// suffix ending `1.2.3/../../../..` passes the prefix check and
/// then [`super::join_global_virtual_store_path`] walks it right
/// back out of the store, because that function's job is to split a
/// suffix into components rather than to judge them.
fn is_graph_node_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The snapshots a cached map has to describe, and the metadata
/// that says how each one's slot path begins.
#[derive(Clone, Copy)]
pub(super) struct Expected<'a> {
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
}

impl Expected<'_> {
    /// `<scope>/<name>/<version>/` — the part of a slot suffix that
    /// follows from the snapshot key alone, leaving only the graph
    /// hash unverified. `None` for a key the caller does not hold,
    /// which fails the entry.
    fn slot_prefix(&self, package_key: &PackageKey) -> Option<String> {
        let metadata_key = package_key.without_peer();
        self.snapshots.get(package_key)?;
        let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
        let name = metadata_key.name.to_string();
        let version = super::gvs_version_segment(metadata, &metadata_key.suffix);
        // The empty digest leaves exactly the fixed part of a
        // suffix: `<scope>/<name>/<version>/`.
        Some(super::format_global_virtual_store_path(&name, &version, ""))
    }
}

fn read_field(bytes: &[u8], cursor: usize) -> Option<(&str, usize)> {
    let len_end = cursor.checked_add(4)?;
    let len = u32::from_le_bytes(bytes.get(cursor..len_end)?.try_into().ok()?) as usize;
    let field_end = len_end.checked_add(len)?;
    let field = std::str::from_utf8(bytes.get(len_end..field_end)?).ok()?;
    Some((field, field_end))
}

/// Best-effort write: a cache that cannot be written is a slower
/// install, never a failed one.
pub(super) fn store(file: CacheFile<'_>, suffixes: &HashMap<PackageKey, String>) {
    let path = cache_path(file.cache_dir, file.lockfile_dir);
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let mut bytes = Vec::with_capacity(suffixes.len() * 192);
    write_field(&mut bytes, file.fingerprint);
    for (package_key, suffix) in suffixes {
        write_field(&mut bytes, &package_key.to_string());
        write_field(&mut bytes, suffix);
    }
    // Staged under a name only this writer knows, then renamed, so
    // a concurrent reader never observes a half-written map and
    // two concurrent writers never share a staging file. A
    // predictable one would also let anything that can write the
    // cache directory redirect the write through a symlink.
    let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) else { return };
    // No `sync_all`: this runs on the miss path, after the map has
    // already been derived, and an fsync there is latency spent on
    // the install this cache exists to speed up. A crash mid-write
    // costs a torn entry, which `load` refuses and re-derives.
    if file.write_all(&bytes).is_ok() {
        let _ = file.persist(&path);
    }
}

fn write_field(bytes: &mut Vec<u8>, field: &str) {
    bytes.extend_from_slice(&(field.len() as u32).to_le_bytes());
    bytes.extend_from_slice(field.as_bytes());
}

//! Seed the runtime storage with the fixture tarballs and metadata
//! database that ship inside [`registry_mock_storage`].
//!
//! The runtime storage outlives the fixtures and may be served by mocks of
//! other fixture builds at once, such as from another worktree. So each
//! fixture build gets a hosted store of its own, [`runtime_hosted_storage`],
//! while every build shares the proxy cache, [`runtime_cache`].
//!
//! Idempotent: a tarball that already exists at the destination is
//! left alone. Tries `hard_link` first (zero disk overhead, instant);
//! falls back to `copy` if hard-linking isn't supported on the
//! target filesystem (cross-device, ACL, etc.).
//!
//! The metadata database is the exception. pnpr, and a benchmark scenario
//! that seeds packages of its own, write to the database the mock serves, so
//! it is copied rather than linked, once, and never replaced under a mock that
//! has it open.

use crate::{registry_mock_storage, runtime_storage};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// The proxy cache's directory in a storage directory, as pnpr defaults it.
const CACHE_DIR: &str = ".pnpr-cache";
use walkdir::WalkDir;

/// Mirror every file under `registry_mock_storage()` into
/// [`runtime_hosted_storage`]. Returns the number of files newly seeded
/// (existing files don't count).
pub fn seed_runtime_storage() -> io::Result<usize> {
    let src = registry_mock_storage();
    let dest = runtime_hosted_storage();
    let dest = dest.as_path();
    if !src.exists() {
        // `registry_mock_storage` builds the fixture storage on first
        // call, so this is unreachable in practice; guard anyway and
        // let pnpr's own startup surface any problem.
        return Ok(0);
    }
    fs::create_dir_all(dest)?;
    let database = pnpr_fixtures::metadata_path(Path::new(""));
    let mut seeded = usize::from(copy_database(&pnpr_fixtures::metadata_path(src))?);
    for entry in WalkDir::new(src) {
        // Propagate traversal errors instead of silently dropping
        // them: a partial-mirror under a "success" return is much
        // harder to debug than an explicit failure.
        let entry = entry.map_err(io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("entry under src");
        // Test registries over the fixture storage cache upstream packages
        // in it. The mock caches in the shared runtime cache instead.
        if rel.starts_with(CACHE_DIR) {
            continue;
        }
        if seed_file(entry.path(), rel, &dest.join(rel), &database)? {
            seeded += 1;
        }
    }
    Ok(seeded)
}

/// The proxy cache every fixture build's mock shares, so a warm cache
/// survives a change to the fixtures.
#[must_use]
pub fn runtime_cache() -> PathBuf {
    runtime_storage().join(CACHE_DIR)
}

/// The hosted store the mock serves for the fixture build
/// [`registry_mock_storage`] holds, named after that build.
#[must_use]
pub fn runtime_hosted_storage() -> PathBuf {
    let build =
        registry_mock_storage().file_name().expect("the fixture storage is named after its build");
    runtime_storage().join(".registry-mock").join(build)
}

/// Where every mock's config is written, beside [`runtime_hosted_storage`]:
/// the config's relative `./htpasswd`, and the tokens beside it, resolve
/// here.
#[must_use]
pub fn runtime_config_dir() -> PathBuf {
    runtime_hosted_storage()
        .parent()
        .expect("the hosted store has a directory")
        .to_path_buf()
}

/// The metadata database of [`runtime_hosted_storage`].
#[must_use]
pub fn runtime_metadata_path() -> PathBuf {
    pnpr_fixtures::metadata_path(&runtime_hosted_storage())
}

/// Seed the fixture file at `src`, at `rel` in the fixture storage, to
/// `dest`, and report whether it was new there. The fixture database and its
/// write-ahead log and shared memory are left to [`copy_database`].
fn seed_file(src: &Path, rel: &Path, dest: &Path, database: &Path) -> io::Result<bool> {
    if rel.parent() == database.parent() {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    // We don't pre-check `dest.exists()` — a TOCTOU window is wide enough
    // on a shared CI worker (two pacquet processes racing past `GuardFile`)
    // that the existence check could pass and then `hard_link` still trip
    // `AlreadyExists`. Treat that as success directly inside `link_or_copy`.
    Ok(matches!(link_or_copy(src, dest)?, LinkOutcome::Created))
}

/// Copy the fixture database at `src` to [`runtime_metadata_path`] unless it
/// is there already, and report whether it was copied. The copy lands under
/// a temporary name and is linked into place, which fails rather than
/// replace a database another seeder put there first, so a pnpr never opens
/// half a database or has its database swapped under it.
fn copy_database(src: &Path) -> io::Result<bool> {
    static COPIES: AtomicU64 = AtomicU64::new(0);
    let dest = runtime_metadata_path();
    if dest.is_file() {
        return Ok(false);
    }
    let parent = dest.parent().expect("the database has a directory");
    fs::create_dir_all(parent)?;
    let mut temp = dest.as_os_str().to_owned();
    let copy = COPIES.fetch_add(1, Ordering::Relaxed);
    temp.push(format!(".seed-{}-{copy}", std::process::id()));
    let temp = PathBuf::from(temp);
    fs::copy(src, &temp)?;
    let linked = fs::hard_link(&temp, &dest);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

enum LinkOutcome {
    Created,
    AlreadyExists,
}

fn link_or_copy(src: &Path, dest: &Path) -> io::Result<LinkOutcome> {
    match fs::hard_link(src, dest) {
        Ok(()) => Ok(LinkOutcome::Created),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(LinkOutcome::AlreadyExists),
        Err(_) => match fs::copy(src, dest) {
            Ok(_) => Ok(LinkOutcome::Created),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                Ok(LinkOutcome::AlreadyExists)
            }
            Err(err) => Err(err),
        },
    }
}

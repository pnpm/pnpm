//! Seed [`runtime_storage`] with the fixture tarballs and metadata
//! database that ship inside [`registry_mock_storage`].
//!
//! Idempotent: a tarball that already exists at the destination is
//! left alone. Tries `hard_link` first (zero disk overhead, instant);
//! falls back to `copy` if hard-linking isn't supported on the
//! target filesystem (cross-device, ACL, etc.).
//!
//! The metadata database is the exception. pnpr, and a benchmark scenario
//! that seeds packages of its own, write to the database the runtime storage
//! serves, so it is copied rather than linked. It is replaced when the
//! fixture storage it was copied from is another one, which is what a change
//! to the fixtures builds: the runtime storage outlives them.

use crate::{registry_mock_storage, runtime_storage};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Mirror every file under `registry_mock_storage()` into
/// `runtime_storage()`. Returns the number of files newly seeded
/// (existing files don't count).
pub fn seed_runtime_storage() -> io::Result<usize> {
    let src = registry_mock_storage();
    let dest = runtime_storage();
    if !src.exists() {
        // `registry_mock_storage` builds the fixture storage on first
        // call, so this is unreachable in practice; guard anyway and
        // let pnpr's own startup surface any problem.
        return Ok(0);
    }
    fs::create_dir_all(dest)?;
    let database = pnpr_fixtures::metadata_path(Path::new(""));
    let mut seeded = 0;
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
        if seed_file(entry.path(), rel, &dest.join(rel), &database)? {
            seeded += 1;
        }
    }
    Ok(seeded)
}

/// Seed the fixture file at `src`, at `rel` in the fixture storage, to
/// `dest`, and report whether it was new there.
fn seed_file(src: &Path, rel: &Path, dest: &Path, database: &Path) -> io::Result<bool> {
    if rel == database {
        return replace_database(src, dest);
    }
    // The database's write-ahead log and shared memory belong to whichever
    // pnpr has the fixture database open.
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

/// Copy the database at `src` over `dest` unless `dest` was copied from
/// `src` already, and report whether it did. The copy lands under a
/// temporary name and is renamed into place, with the log of the database it
/// replaces removed, so a pnpr never opens half a database or applies another
/// database's log to it.
fn replace_database(src: &Path, dest: &Path) -> io::Result<bool> {
    let source = with_suffix(dest, ".source");
    let origin = src.to_string_lossy();
    match fs::read_to_string(&source) {
        Ok(recorded) if recorded == origin && dest.is_file() => return Ok(false),
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let parent = dest.parent().expect("the database has a directory");
    fs::create_dir_all(parent)?;
    let temp = with_suffix(dest, &format!(".seed-{}", std::process::id()));
    fs::copy(src, &temp)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        match fs::remove_file(with_suffix(dest, suffix)) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    fs::rename(&temp, dest)?;
    fs::write(&source, origin.as_bytes())?;
    Ok(true)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
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

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
//! serves, so it is copied rather than linked. The runtime storage outlives
//! the fixtures and may be served by mocks of other fixtures at once, such as
//! from another worktree, so each fixture build gets a database of its own,
//! [`runtime_metadata_path`], copied once and never replaced under a mock
//! that has it open.

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
        if seed_file(entry.path(), rel, &dest.join(rel), &database)? {
            seeded += 1;
        }
    }
    Ok(seeded)
}

/// The metadata database the runtime storage serves for the fixture build
/// [`registry_mock_storage`] holds, named after that build.
#[must_use]
pub fn runtime_metadata_path() -> PathBuf {
    let build =
        registry_mock_storage().file_name().expect("the fixture storage is named after its build");
    let mut name = build.to_owned();
    name.push(".sqlite");
    runtime_storage().join(".registry-mock").join(name)
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
/// a temporary name and is renamed into place, so a pnpr never opens half a
/// database.
fn copy_database(src: &Path) -> io::Result<bool> {
    let dest = runtime_metadata_path();
    if dest.is_file() {
        return Ok(false);
    }
    let parent = dest.parent().expect("the database has a directory");
    fs::create_dir_all(parent)?;
    let mut temp = dest.as_os_str().to_owned();
    temp.push(format!(".seed-{}", std::process::id()));
    let temp = PathBuf::from(temp);
    fs::copy(src, &temp)?;
    match fs::rename(&temp, &dest) {
        Ok(()) => Ok(true),
        // Another seeder won the race, with the same bytes.
        Err(_) if dest.is_file() => {
            let _ = fs::remove_file(&temp);
            Ok(false)
        }
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

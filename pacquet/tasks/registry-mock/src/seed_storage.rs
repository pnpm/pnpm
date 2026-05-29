//! Seed [`runtime_storage`] with the fixture packuments and tarballs
//! that ship inside [`registry_mock_storage`].
//!
//! Idempotent: a file that already exists at the destination is
//! left alone. Tries `hard_link` first (zero disk overhead, instant);
//! falls back to `copy` if hard-linking isn't supported on the
//! target filesystem (cross-device, ACL, etc.).

use crate::{registry_mock_storage, runtime_storage};
use std::{fs, io, path::Path};
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
    let mut seeded = 0;
    for entry in WalkDir::new(src) {
        // Propagate traversal errors instead of silently dropping
        // them: a partial-mirror under a "success" return is much
        // harder to debug than an explicit failure.
        let entry = entry.map_err(io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(src).expect("entry under src");
        let dest_path = dest.join(rel);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // We don't pre-check `dest_path.exists()` — a TOCTOU window
        // is wide enough on a shared CI worker (two pacquet
        // processes racing past `GuardFile`) that the existence
        // check could pass and then `hard_link` still trip
        // `AlreadyExists`. Treat that as success directly inside
        // `link_or_copy`.
        match link_or_copy(entry.path(), &dest_path)? {
            LinkOutcome::Created => seeded += 1,
            LinkOutcome::AlreadyExists => {}
        }
    }
    Ok(seeded)
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

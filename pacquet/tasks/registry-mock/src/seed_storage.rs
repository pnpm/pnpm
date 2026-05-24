//! Seed [`runtime_storage`](crate::runtime_storage) with the fixture
//! packuments and tarballs that ship inside
//! [`registry_mock_storage`](crate::registry_mock_storage).
//!
//! Idempotent: a file that already exists at the destination is
//! left alone. Tries `hard_link` first (zero disk overhead, instant);
//! falls back to `copy` if hard-linking isn't supported on the
//! target filesystem (cross-device, ACL, etc.).

use crate::{registry_mock_storage, runtime_storage};
use std::fs;
use std::io;
use std::path::Path;
use walkdir::WalkDir;

/// Mirror every file under `registry_mock_storage()` into
/// `runtime_storage()`. Returns the number of files newly seeded
/// (existing files don't count).
pub fn seed_runtime_storage() -> io::Result<usize> {
    let src = registry_mock_storage();
    let dest = runtime_storage();
    if !src.exists() {
        // The launcher needs registry-mock installed via pnpm — if
        // it isn't, fall through with zero seeded and let
        // pnpm-registry's own startup fail with a clear message.
        return Ok(0);
    }
    fs::create_dir_all(dest)?;
    let mut seeded = 0;
    for entry in WalkDir::new(src).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(src).expect("entry under src");
        let dest_path = dest.join(rel);
        if dest_path.exists() {
            continue;
        }
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        link_or_copy(entry.path(), &dest_path)?;
        seeded += 1;
    }
    Ok(seeded)
}

fn link_or_copy(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::hard_link(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dest).map(|_| ()),
    }
}

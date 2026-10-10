use crate::runtime_storage_root;
use pnpr_fixtures::FixtureGeneration;
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

const GENERATION_FILE: &str = ".fixture-generation";

#[derive(Debug)]
pub struct RuntimeStorage {
    path: PathBuf,
    seeded_files: usize,
}

impl RuntimeStorage {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn seeded_files(&self) -> usize {
        self.seeded_files
    }
}

pub fn seed_runtime_storage() -> io::Result<RuntimeStorage> {
    seed_generation(pnpr_fixtures::current(), runtime_storage_root())
}

fn seed_generation(generation: &FixtureGeneration, root: &Path) -> io::Result<RuntimeStorage> {
    let path = root.join(generation.fingerprint());
    let marker = path.join(GENERATION_FILE);
    if let Some(seeded_for) = read_generation_marker(&marker)? {
        if seeded_for != generation.fingerprint() {
            return Err(marker_generation_mismatch(&marker, &seeded_for, generation.fingerprint()));
        }
        return Ok(RuntimeStorage { path, seeded_files: 0 });
    }
    let seeded_files = seed_files(generation, &path)?;
    publish_generation_marker(&marker, generation.fingerprint())?;
    Ok(RuntimeStorage { path, seeded_files })
}

fn publish_generation_marker(marker: &Path, fingerprint: &str) -> io::Result<()> {
    let parent = marker.parent().expect("generation marker has a parent directory");
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(fingerprint.as_bytes())?;
    match temporary.persist_noclobber(marker) {
        Ok(_) => Ok(()),
        Err(err) if err.error.kind() == io::ErrorKind::AlreadyExists => {
            match read_generation_marker(marker)? {
                Some(seeded_for) if seeded_for == fingerprint => Ok(()),
                Some(seeded_for) => {
                    Err(marker_generation_mismatch(marker, &seeded_for, fingerprint))
                }
                None => Err(err.error),
            }
        }
        Err(err) => Err(err.error),
    }
}

fn marker_generation_mismatch(marker: &Path, seeded_for: &str, expected: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "runtime storage {} is seeded for fixture generation {seeded_for}, not {expected}: \
             point PNPM_REGISTRY_STORAGE at a directory this mock owns",
            marker
                .parent()
                .unwrap_or(marker)
                .display(),
        ),
    )
}

fn read_generation_marker(marker: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(marker) {
        Ok(seeded_for) => Ok(Some(seeded_for.trim_end().to_string())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn seed_files(generation: &FixtureGeneration, dest: &Path) -> io::Result<usize> {
    let src = generation.storage();
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
        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("entry under src");
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
        if matches!(link_or_copy(entry.path(), &dest_path)?, LinkOutcome::Created) {
            seeded += 1;
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
        Err(_) => copy_into_new_file(src, dest),
    }
}

fn copy_into_new_file(src: &Path, dest: &Path) -> io::Result<LinkOutcome> {
    let mut source = fs::File::open(src)?;
    let parent = dest.parent().expect("runtime storage file has a parent directory");
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    io::copy(&mut source, &mut temporary)?;
    temporary
        .as_file()
        .set_permissions(source.metadata()?.permissions())?;
    match temporary.persist_noclobber(dest) {
        Ok(_) => Ok(LinkOutcome::Created),
        Err(err) if err.error.kind() == io::ErrorKind::AlreadyExists => {
            Ok(LinkOutcome::AlreadyExists)
        }
        Err(err) => Err(err.error),
    }
}

#[cfg(test)]
mod tests;

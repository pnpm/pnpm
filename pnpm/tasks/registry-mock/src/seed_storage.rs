//! Reconcile the runtime storage a mock serves from with one fixture
//! generation's storage.
//!
//! `pnpr` is pointed at a runtime directory rather than at the generated
//! fixture storage so the proxy-cache entries it writes there survive a
//! fixture rebuild. That directory therefore holds content no seed owns,
//! which the reconcile below has to leave alone.

use crate::runtime_storage_root;
use pnpr_fixtures::FixtureGeneration;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Which fixture generation a runtime directory was seeded for. Written
/// only once the seed completes, so its absence means the directory holds
/// an unfinished generation.
const GENERATION_FILE: &str = ".fixture-generation";

/// A runtime storage directory reconciled to one fixture generation, and
/// so safe to hand `pnpr` as `--storage`.
///
/// [`seed_runtime_storage`] is the only constructor, so a directory
/// reaches `pnpr` only once the generation being launched is in it. The
/// mock therefore cannot be pointed at another generation's packuments
/// without [`seed_runtime_storage`] doing so on purpose.
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

    /// How many files this launch added. Zero once a generation's
    /// directory is seeded, which is every launch after its first.
    #[must_use]
    pub fn seeded_files(&self) -> usize {
        self.seeded_files
    }
}

/// Seed the committed fixtures' generation into its own directory under
/// the runtime storage root.
pub fn seed_runtime_storage() -> io::Result<RuntimeStorage> {
    seed_generation(pnpr_fixtures::current(), runtime_storage_root())
}

// Keying the directory by fingerprint is what keeps a launch from serving
// another generation: seeding a directory shared by every generation adds
// files and removes none, so a packument the fixtures have since changed
// would survive on disk and `pnpr` would serve it as authoritative. A path
// named for the fingerprint can only hold the files that fingerprint covers.
/// Reconcile `root`'s directory for `generation` with that generation's
/// storage, and return it once it holds that generation's files alongside
/// whatever `pnpr` has cached there. The directory is `<root>/<fingerprint>`.
///
/// Reconciliation is additive and deletes nothing. The proxy cache and the
/// packages a benchmark scenario seeds in are not the generation's to
/// remove, so a repeat launch of one generation keeps both.
///
/// # Errors
///
/// Fails if the directory is marked for another generation, meaning
/// `PNPM_REGISTRY_STORAGE` points at a directory that is not a runtime
/// root — one already holding a per-generation subdirectory for other
/// fixtures, where seeding would serve whichever generation wrote first.
fn seed_generation(generation: &FixtureGeneration, root: &Path) -> io::Result<RuntimeStorage> {
    let path = root.join(generation.fingerprint());
    let marker = path.join(GENERATION_FILE);
    if let Some(seeded_for) = read_generation_marker(&marker)? {
        if seeded_for != generation.fingerprint() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "runtime storage {} is seeded for fixture generation {seeded_for}, not {}: \
                     point PNPM_REGISTRY_STORAGE at a directory this mock owns",
                    path.display(),
                    generation.fingerprint(),
                ),
            ));
        }
        return Ok(RuntimeStorage { path, seeded_files: 0 });
    }
    let seeded_files = seed_files(generation, &path)?;
    fs::write(&marker, generation.fingerprint())
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("write fixture generation marker at {}: {err}", marker.display()),
            )
        })?;
    Ok(RuntimeStorage { path, seeded_files })
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
        // Hard links fail across devices and under some ACLs. The copy claims
        // the destination the way `hard_link` does, so both paths agree an
        // existing file is left alone; `fs::copy` would truncate it instead.
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

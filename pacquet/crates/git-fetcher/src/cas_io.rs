//! CAS I/O helpers shared between [`crate::GitFetcher`] (git clone +
//! `preparePackage`) and [`crate::GitHostedTarballFetcher`] (tarball
//! download + `preparePackage`).
//!
//! - [`materialize_into`] copies CAS-resident files into a fresh
//!   working directory so the prepare phase has a writable tree.
//!   Used by the git-hosted tarball fetcher: by the time the tarball
//!   has been downloaded by `pacquet-tarball`, the files already live
//!   in the CAS, so the prepare phase reads them out into a temp dir
//!   it can mutate without corrupting the CAS.
//! - [`import_into_cas`] writes a prepared file set back to the CAS
//!   and produces the `relative-path → cas-path` map the install
//!   dispatcher hands to `CreateVirtualDirBySnapshot`.
//! - [`map_write_cas`] is a minor helper factored out alongside the
//!   import.

use crate::error::GitFetcherError;
use pacquet_fs::file_mode::{
    cas_path_is_executable, is_executable, restore_exec_bit_from_cas_suffix,
};
use pacquet_store_dir::{CafsFileInfo, StoreDir};
use std::{
    collections::HashMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Result of [`import_into_cas`]. The dispatcher uses `cas_paths` to
/// build the virtual-store layout; `files_index` is the
/// [`pacquet_store_dir::PackageFilesIndex::files`] payload the warm
/// prefetch on a future install reads to skip re-fetching.
pub(crate) struct ImportedFiles {
    pub cas_paths: HashMap<String, PathBuf>,
    pub files_index: HashMap<String, CafsFileInfo>,
}

/// Safely join a relative path onto a trusted root, rejecting anything
/// that wouldn't stay under `root`.
///
/// Both `materialize_into` and `import_into_cas` receive their
/// relative paths from the install dispatcher's `cas_paths` map,
/// which traces back to either a tarball extraction or a packlist
/// over a freshly-checked-out git tree. Tarball entries on the
/// extraction side already get path-traversal guards in
/// `pacquet-tarball`, but defense-in-depth at this layer means a
/// future caller (or a bug in the upstream sanitiser) can't turn
/// a malformed entry into a write outside the working tree.
fn join_checked(root: &Path, rel: &str) -> Result<PathBuf, GitFetcherError> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(GitFetcherError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("absolute path is not allowed in CAS entry: {rel}"),
        )));
    }
    let mut out = root.to_path_buf();
    for c in rel_path.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(GitFetcherError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("non-normal path component in CAS entry: {rel}"),
                )));
            }
        }
    }
    Ok(out)
}

/// Copy every CAS file referenced in `cas_paths` into `target_dir`,
/// preserving relative paths. CAS files are hardlinked-or-copied per
/// install elsewhere, but for the prepare phase the working tree must
/// be writable *without* mutating the shared CAS entry, so this path
/// always allocates fresh inodes via [`fs::copy`].
///
/// Mirrors the effect of upstream's
/// [`cafs.importPackage(tempLocation, …)`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/src/gitHostedTarballFetcher.ts#L75)
/// call inside `prepareGitHostedPkg`, but produces a *standalone*
/// directory rather than a pnpm-style CAFS slot — pacquet's `StoreDir`
/// only knows how to import on the way *in*, and the prepare phase
/// needs raw filesystem semantics for scripts to run.
pub(crate) fn materialize_into(
    cas_paths: &HashMap<String, PathBuf>,
    target_dir: &Path,
) -> Result<(), GitFetcherError> {
    for (rel, cas_path) in cas_paths {
        // `rel` uses forward slashes regardless of host platform.
        // `Path::components()` (called inside `join_checked`)
        // recognises both `/` and `\` as separators on Windows, so we
        // can hand `rel` over directly and avoid a per-file `String`
        // allocation.
        let target = join_checked(target_dir, rel)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(GitFetcherError::Io)?;
        }
        fs::copy(cas_path, &target).map_err(GitFetcherError::Io)?;
        restore_exec_bit_from_cas_suffix(cas_path, &target).map_err(GitFetcherError::Io)?;
    }
    Ok(())
}

/// Write each file in `files` (relative to `pkg_dir`) into the CAS,
/// returning both the install-dispatcher map and the
/// [`pacquet_store_dir::PackageFilesIndex::files`] payload the fetcher
/// queues to `index.db` so a future install's warm prefetch can skip
/// the re-fetch. Mirrors the role of upstream's
/// [`addFilesFromDir`](https://github.com/pnpm/pnpm/blob/94240bc046/store/cafs/src/addFilesFromDir.ts)
/// on the post-prepare write side.
pub(crate) fn import_into_cas(
    store_dir: &StoreDir,
    pkg_dir: &Path,
    files: &[String],
) -> Result<ImportedFiles, GitFetcherError> {
    let mut cas_paths = HashMap::with_capacity(files.len());
    let mut files_index = HashMap::with_capacity(files.len());
    for rel in files {
        // See the matching note in `materialize_into`: `join_checked`
        // accepts forward-slash relative paths verbatim on every host.
        let source = join_checked(pkg_dir, rel)?;
        let metadata = fs::metadata(&source).map_err(GitFetcherError::Io)?;
        let mode = file_mode_from(&metadata);
        // `add_files_from_dir` reads the full POSIX mode and routes
        // the executable bit through `is_executable(mode)`. Match
        // that so a git-hosted snapshot's `PackageFilesIndex.files`
        // round-trips through pacquet's read side at
        // `cas_file_path_by_mode` exactly like a tarball entry would.
        let bytes = fs::read(&source).map_err(GitFetcherError::Io)?;
        let size = bytes.len() as u64;
        let executable = is_executable(mode);
        let (cas_path, hash) =
            store_dir.write_cas_file(&bytes, executable).map_err(map_write_cas)?;
        cas_paths.insert(rel.clone(), cas_path);
        files_index.insert(
            rel.clone(),
            CafsFileInfo {
                digest: format!("{hash:x}"),
                mode,
                size,
                // `None` matches `add_files_from_dir`: the first
                // integrity-check pass populates this. Staying
                // consistent with the existing CAFS write path means
                // the warm prefetch's verify logic exercises the
                // same code path it does for tarball entries.
                checked_at: None,
            },
        );
    }
    Ok(ImportedFiles { cas_paths, files_index })
}

/// POSIX file mode (`meta.mode() & 0o777`) on Unix; a fixed `0o644`
/// on Windows where the OS has no analog. Mirrors
/// `pacquet_store_dir::add_files_from_dir::file_mode_from`.
#[cfg(unix)]
fn file_mode_from(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode_from(_meta: &fs::Metadata) -> u32 {
    0o644
}

/// Synthesize the [`PackageFilesIndex::files`](pacquet_store_dir::PackageFilesIndex::files)
/// payload from an existing `cas_paths` map without re-reading file
/// bytes. Used by [`crate::GitHostedTarballFetcher`]'s fast path
/// (mirrors upstream's [`gitHostedTarballFetcher.ts:88-100`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/src/gitHostedTarballFetcher.ts#L88-L100)),
/// where the prepared file set is byte-identical to the raw tarball,
/// so re-hashing every entry into the CAS would be wasted work.
///
/// The digest is extracted from the CAS path itself — pnpm v11 lays
/// CAS files out as `files/XX/<rest>[-exec]`, so concatenating the
/// shard byte (parent directory name) with the file stem (sans the
/// optional `-exec` suffix) reconstructs the full hex digest. Mode
/// reporting follows the read side's
/// [`cas_file_path_by_mode`](pacquet_store_dir::StoreDir::cas_file_path_by_mode)
/// rule: any-exec-bit-set ↔ `-exec` suffix, so a synthesized `0o755`
/// for `-exec` entries and `0o644` otherwise round-trip cleanly.
/// `size` still requires a `fs::metadata` stat, but that's two syscalls
/// per file rather than a full read + sha512.
pub(crate) fn synthesize_files_index(
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<HashMap<String, CafsFileInfo>, GitFetcherError> {
    let mut out = HashMap::with_capacity(cas_paths.len());
    for (rel, cas_path) in cas_paths {
        let digest = cas_path_digest(cas_path).ok_or_else(|| {
            GitFetcherError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CAS path {cas_path:?} for {rel:?} does not match `files/XX/<rest>[-exec]`",
                ),
            ))
        })?;
        let executable = cas_path_is_executable(cas_path);
        // Match the read-side `cas_file_path_by_mode` round-trip rule:
        // any-exec-bit-set ↔ `-exec` suffix. The exact mode value is
        // only consulted by `is_executable`, so `0o755` / `0o644` are
        // canonical representatives — they replay through the same
        // `-exec` decision the write side made for these blobs.
        let mode = if executable { 0o755 } else { 0o644 };
        let metadata = fs::metadata(cas_path).map_err(GitFetcherError::Io)?;
        let size = metadata.len();
        out.insert(rel.clone(), CafsFileInfo { digest, mode, size, checked_at: None });
    }
    Ok(out)
}

/// Reconstruct the hex digest of a CAS file from its path. The pnpm
/// v11 layout puts CAS files at `<store>/v11/files/<XX>/<rest>[-exec]`
/// where `<XX>` is the first byte of the sha512 hex digest and
/// `<rest>` is the remaining 126 hex characters. Returns `None` when
/// the path doesn't match that shape, so callers can surface a clear
/// error rather than silently producing a malformed digest.
fn cas_path_digest(path: &Path) -> Option<String> {
    // SHA-512 produces 64 bytes / 128 hex chars. The first 2 hex
    // chars become the shard directory; the rest become the file
    // stem. Anything outside that exact shape is not a v11 CAS path
    // — fail closed instead of producing a short / over-long digest
    // that would later poison `index.db`.
    const STEM_LEN: usize = 128 - 2;
    let file_name = path.file_name()?.to_str()?;
    let stem = file_name.strip_suffix("-exec").unwrap_or(file_name);
    let shard = path.parent()?.file_name()?.to_str()?;
    if shard.len() != 2 || !shard.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if stem.len() != STEM_LEN || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("{shard}{stem}"))
}

/// Re-wrap a CAFS write failure as a `GitFetcherError::AddFilesFromDir`.
/// Preserves the miette source chain shape so a future replacement of
/// the per-file `write_cas_file` loop with an `add_files_from_dir`-
/// shaped helper doesn't disturb the dispatcher's error rendering.
pub(crate) fn map_write_cas(err: pacquet_store_dir::WriteCasFileError) -> GitFetcherError {
    let pacquet_store_dir::WriteCasFileError::WriteFile(inner) = err;
    GitFetcherError::AddFilesFromDir(pacquet_store_dir::AddFilesFromDirError::WriteCas(
        pacquet_store_dir::WriteCasFileError::WriteFile(inner),
    ))
}

#[cfg(test)]
mod tests;

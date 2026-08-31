//! Reading tarballs already on disk.
//!
//! Covers `file:` tarball payloads and the metadata probes that read a
//! local archive without going through the store.

use super::{
    Component, Cursor, HashMap, MAX_UNTRUSTED_PREALLOC_BYTES, Path, PathBuf, Read, TarballError,
    allocate_tarball_buffer, decompress_gzip, io, is_eager_decode_limit_exceeded,
    normalize_bundled_manifest, oversized_manifest_error, post_download_semaphore,
    tar_entry_payload, verify_tarball_integrity,
};
use pnpm_package_manifest::parse_manifest_bytes;
use ssri::Integrity;
use tar::Archive;

pub(crate) async fn open_local_tarball(
    path: &Path,
) -> Result<(tokio::fs::File, u64), TarballError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|source| TarballError::ReadLocalTarball { path: path.to_path_buf(), source })?;
    reject_non_file_local_tarball(path, &metadata)?;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| TarballError::ReadLocalTarball { path: path.to_path_buf(), source })?;
    let metadata = file
        .metadata()
        .await
        .map_err(|source| TarballError::ReadLocalTarball { path: path.to_path_buf(), source })?;
    reject_non_file_local_tarball(path, &metadata)?;
    Ok((file, metadata.len()))
}

pub(crate) fn reject_non_file_local_tarball(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), TarballError> {
    if metadata.is_file() {
        return Ok(());
    }
    Err(read_local_tarball_error(
        path,
        io::ErrorKind::InvalidInput,
        "local tarball path is not a regular file",
    ))
}

pub(crate) async fn read_local_tarball_buffer(
    file: tokio::fs::File,
    path: &Path,
    package_url: &str,
    size: u64,
) -> Result<Vec<u8>, TarballError> {
    use tokio::io::AsyncReadExt;

    let read_limit = size.checked_add(1).ok_or_else(|| {
        read_local_tarball_error(
            path,
            io::ErrorKind::InvalidData,
            format!("local tarball is too large to read into memory ({size} bytes)"),
        )
    })?;
    let mut buffer = allocate_local_tarball_buffer(path, package_url, size)?;
    let mut reader = file.take(read_limit);
    reader
        .read_to_end(&mut buffer)
        .await
        .map_err(|source| TarballError::ReadLocalTarball { path: path.to_path_buf(), source })?;
    if u64::try_from(buffer.len()).unwrap_or(u64::MAX) > size {
        return Err(read_local_tarball_error(
            path,
            io::ErrorKind::InvalidData,
            format!("local tarball changed while reading; refused to read past {size} bytes"),
        ));
    }
    Ok(buffer)
}

pub(crate) fn allocate_local_tarball_buffer(
    path: &Path,
    package_url: &str,
    size: u64,
) -> Result<Vec<u8>, TarballError> {
    allocate_tarball_buffer(Some(size), package_url).map_err(|error| match error {
        TarballError::TarballTooLarge { .. } => read_local_tarball_error(
            path,
            io::ErrorKind::InvalidData,
            format!("local tarball is too large to read into memory ({size} bytes)"),
        ),
        other => other,
    })
}

pub(crate) fn read_local_tarball_error(
    path: &Path,
    kind: io::ErrorKind,
    message: impl Into<String>,
) -> TarballError {
    TarballError::ReadLocalTarball {
        path: path.to_path_buf(),
        source: io::Error::new(kind, message.into()),
    }
}

pub(crate) fn local_file_tarball_path(package_url: &str) -> Option<PathBuf> {
    let path = package_url.strip_prefix("file:")?;
    if is_unc_like_file_payload(path) {
        return None;
    }
    if path.starts_with('/')
        && let Ok(url) = url::Url::parse(package_url)
    {
        if url.scheme() != "file" || url.has_host() {
            return None;
        }
        let path = url.to_file_path().ok()?;
        return (!is_unc_like_file_payload(&path.to_string_lossy())).then_some(path);
    }
    Some(PathBuf::from(path))
}

pub(crate) fn is_unc_like_file_payload(path: &str) -> bool {
    path.starts_with(r"\\")
        || path.starts_with("////")
        || (path.starts_with("//") && !path.starts_with("///"))
}

/// Read `<subdir>/package.json` out of a freshly extracted archive.
///
/// Extraction only stashes the *root* `package.json` on the
/// [`pnpm_store_dir::PackageFilesIndex`], so a package living in a subdirectory of the
/// archive has to be read back from the CAS. Returns `None` when the
/// subdirectory has no `package.json`, matching the root path's
/// best-effort contract — the caller degrades rather than failing the
/// resolve.
pub(crate) async fn read_subdir_manifest(
    cas_paths: &HashMap<String, PathBuf>,
    subdir: &str,
) -> Result<Option<serde_json::Value>, TarballError> {
    // `cas_paths` is keyed by the archive-relative path left after the
    // top-level prefix strip; the resolution's `path` keeps the leading
    // slash it was written with (`#path:/packages/foo`).
    let key = format!("{}/package.json", subdir.trim_matches('/'));
    let Some(cas_path) = cas_paths.get(&key) else { return Ok(None) };
    let bytes = tokio::fs::read(cas_path)
        .await
        .map_err(|source| TarballError::ReadLocalTarball { path: cas_path.clone(), source })?;
    match parse_manifest_bytes(&bytes) {
        Ok(parsed) => Ok(normalize_bundled_manifest(&parsed)),
        Err(error) => {
            tracing::debug!(
                ?error,
                ?key,
                "package.json in archive subdirectory failed to parse as JSON; bundled manifest cleared",
            );
            Ok(None)
        }
    }
}

/// Outcome of [`read_local_tarball_metadata`]: the sha512 integrity
/// computed from the tarball's bytes and the bundled manifest read from
/// its root `package.json`.
#[derive(Debug)]
pub struct LocalTarballMetadata {
    pub integrity: Integrity,
    /// `None` when the narrowing kept nothing — the archive has no root
    /// `package.json`, or one that is not a JSON object, or one whose
    /// every field was dropped.
    pub manifest: Option<serde_json::Value>,
    /// Whether the archive carried a root `package.json` at all,
    /// regardless of what survived the narrowing.
    ///
    /// The two shapes behind a `None` manifest call for opposite
    /// handling, and only the reader can tell them apart: pnpm installs
    /// an archive that ships no manifest (synthesizing a name from the
    /// alias) but refuses one whose manifest names no package
    /// (`ERR_PNPM_MISSING_PACKAGE_NAME`).
    pub has_manifest_entry: bool,
}

/// Read a local tarball's sha512 integrity and bundled manifest during
/// *resolution*.
///
/// A `file:` tarball dependency carries no name, version, or integrity
/// in its specifier — those live in the archive's own `package.json` —
/// and pacquet builds the lockfile before the install pass runs, so the
/// local resolver has to read them here.
///
/// Nothing is written to the store, unlike the remote-tarball sibling
/// [`crate::FetchTarballForResolution`]: the install pass addresses a `file:`
/// tarball's store-index row by its `<name>@file:<path>` dep path, not
/// by the `<name>@<version>` a resolve-time extraction could key, so a
/// row written here would never be read.
pub async fn read_local_tarball_metadata(
    path: &Path,
) -> Result<LocalTarballMetadata, TarballError> {
    let package_url = format!("file:{}", path.display());
    // pnpm names the plain filesystem path — not the `file:` URL — when
    // it reports a bad tarball, so the manifest error quotes the same.
    let tarball_path = path.display().to_string();
    let (file, size) = open_local_tarball(path).await?;
    let buffer = read_local_tarball_buffer(file, path, &package_url, size).await?;

    let _post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");
    tokio::task::spawn_blocking(move || {
        let integrity = verify_tarball_integrity(&buffer, None, package_url)?;
        let (manifest, has_manifest_entry) =
            read_bundled_manifest_from_archive(&buffer, &tarball_path)?;
        Ok(LocalTarballMetadata { integrity, manifest, has_manifest_entry })
    })
    .await
    .map_err(TarballError::TaskJoin)?
}

/// Read the root `package.json` out of a gzipped archive, decoding it
/// whole while it is small enough for [`decompress_gzip`] and streaming
/// it past that. See [`crate::extract::extract_gzipped_tarball`] for
/// why the whole-archive decode has a ceiling and why reaching it is
/// not a refusal.
pub(crate) fn read_bundled_manifest_from_archive(
    gz_data: &[u8],
    tarball_path: &str,
) -> Result<(Option<serde_json::Value>, bool), TarballError> {
    match decompress_gzip(gz_data, None) {
        Ok(tar_data) => read_bundled_manifest(&tar_data, tarball_path),
        Err(error) if is_eager_decode_limit_exceeded(&error) => {
            read_bundled_manifest_streaming(flate2::read::GzDecoder::new(gz_data), tarball_path)
        }
        Err(error) => Err(error),
    }
}

/// Read the root `package.json` out of a decompressed tar stream,
/// narrowed by [`normalize_bundled_manifest`] so it matches the
/// manifest an extraction stashes on [`pnpm_store_dir::PackageFilesIndex`].
///
/// Shares the entry conventions of
/// [`crate::extract::extract_tarball_entries`] but deliberately not its
/// error handling: there an unparsable `package.json` degrades to
/// `None`, since install-side consumers re-read it from disk. Here it
/// is the only thing naming the package, and an unnamed package
/// resolves to a dep path no lockfile key parses, so it raises
/// [`TarballError::ParseBundledManifest`].
///
/// The returned flag reports whether a root `package.json` existed,
/// which `None` alone cannot: narrowing also yields `None` for a
/// non-object manifest, or one whose every field was dropped.
pub(crate) fn read_bundled_manifest(
    tar_data: &[u8],
    tarball_path: &str,
) -> Result<(Option<serde_json::Value>, bool), TarballError> {
    let mut archive = Archive::new(Cursor::new(tar_data));
    let mut payload = None;
    for entry in archive.entries_with_seek().map_err(TarballError::ReadTarballEntries)? {
        let entry = entry.map_err(TarballError::ReadTarballEntries)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(TarballError::ReadTarballEntries)?;
        if !is_root_manifest_entry_path(&path) {
            continue;
        }
        drop(path);
        // Only the surviving entry is parsed, so a malformed duplicate
        // that a later one supersedes can't fail the read.
        payload = Some(tar_entry_payload(tar_data, &entry)?);
    }
    let Some(payload) = payload else { return Ok((None, false)) };
    finish_bundled_manifest(payload, tarball_path)
}

/// [`read_bundled_manifest`] over a tar stream that is not held in
/// memory: only the manifest entry itself is buffered, so the archive's
/// size no longer bounds this read.
fn read_bundled_manifest_streaming(
    reader: impl Read,
    tarball_path: &str,
) -> Result<(Option<serde_json::Value>, bool), TarballError> {
    let mut archive = Archive::new(reader);
    let mut payload: Option<Vec<u8>> = None;
    for entry in archive.entries().map_err(TarballError::ReadTarballEntries)? {
        let mut entry = entry.map_err(TarballError::ReadTarballEntries)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let is_manifest = {
            let path = entry.path().map_err(TarballError::ReadTarballEntries)?;
            is_root_manifest_entry_path(&path)
        };
        if !is_manifest {
            continue;
        }
        let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
        if file_size > MAX_UNTRUSTED_PREALLOC_BYTES as u64 {
            return Err(oversized_manifest_error(file_size));
        }
        let mut data = Vec::with_capacity(file_size as usize);
        entry.read_to_end(&mut data).map_err(TarballError::ReadTarballEntries)?;
        payload = Some(data);
    }
    let Some(payload) = payload else { return Ok((None, false)) };
    finish_bundled_manifest(&payload, tarball_path)
}

/// Whether an archive entry is the package's own `package.json` — the
/// one directly inside the top-level directory every published tarball
/// wraps its payload in, not a `package.json` shipped in a subdirectory.
fn is_root_manifest_entry_path(path: &Path) -> bool {
    let mut components = path.components().skip(1);
    components.next() == Some(Component::Normal("package.json".as_ref()))
        && components.next().is_none()
}

fn finish_bundled_manifest(
    payload: &[u8],
    tarball_path: &str,
) -> Result<(Option<serde_json::Value>, bool), TarballError> {
    let parsed = parse_manifest_bytes(payload).map_err(|source| {
        TarballError::ParseBundledManifest { tarball: tarball_path.to_string(), source }
    })?;
    Ok((normalize_bundled_manifest(&parsed), true))
}

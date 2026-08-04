//! Tarball decompression and entry extraction into the CAS.

use super::{
    Archive, CafsFileInfo, Component, Cursor, DeflateDecoder, DeflateOptions, HashMap,
    IgnoreEntryFilter, IntoParallelRefIterator, MAX_UNTRUSTED_PREALLOC_BYTES, PackageFilesIndex,
    ParallelIterator, PathBuf, StoreDir, TarballError, UNIX_EPOCH, cas_write_pool, file_mode,
    files_include_install_scripts, instrument, manifest_requires_build, parse_manifest_bytes,
};

/// Build the buffer that the tarball body streams into, pre-sized
/// from the response's advertised `Content-Length` when it fits and
/// can actually be reserved without allocation failure.
///
/// `Content-Length` is untrusted input — a malicious or broken
/// registry could advertise `u64::MAX`, which would crash the
/// process if we passed it directly to `Vec::with_capacity`. Two
/// guards:
///
/// 1. `usize::try_from(size)` — on 32-bit targets a `u64` header
///    value may exceed `usize::MAX`; on 64-bit the two are the
///    same width but the conversion is cheap anyway.
/// 2. `Vec::try_reserve_exact(cap)` — if the allocator refuses
///    (legitimate OOM, or because `cap` is absurdly large relative
///    to available RAM), we surface `TarballTooLarge` instead of
///    aborting via the infallible `with_capacity` path.
///
/// When `content_length` is absent the response uses chunked
/// transfer encoding and we can't pre-size; return an empty
/// growable `Vec` and let the stream loop extend it.
pub(crate) fn allocate_tarball_buffer(
    content_length: Option<u64>,
    url: &str,
) -> Result<Vec<u8>, TarballError> {
    let Some(size) = content_length else {
        return Ok(Vec::new());
    };

    let too_large =
        || TarballError::TarballTooLarge { url: url.to_string(), advertised_size: size };

    let capacity = usize::try_from(size).map_err(|_| too_large())?;
    let mut buf = Vec::new();
    buf.try_reserve_exact(capacity).map_err(|_| too_large())?;
    Ok(buf)
}

/// Bound a registry-supplied `dist.unpackedSize` before it reaches
/// zune-inflate, which reserves the hint as an infallible zero-filled
/// `vec![0; hint]` and aborts the process if that allocation fails.
pub(crate) fn bounded_gzip_size_hint(unpacked_size: Option<usize>) -> Option<usize> {
    unpacked_size.map(|size| size.min(MAX_UNTRUSTED_PREALLOC_BYTES))
}

#[instrument(skip(gz_data), fields(gz_data_len = gz_data.len()))]
pub(crate) fn decompress_gzip(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
) -> Result<Vec<u8>, TarballError> {
    let mut options = DeflateOptions::default().set_confirm_checksum(false);

    if let Some(size) = bounded_gzip_size_hint(unpacked_size) {
        options = options.set_size_hint(size);
    }

    DeflateDecoder::new_with_options(gz_data, options)
        .decode_gzip()
        .map_err(TarballError::DecodeGzip)
}

/// Pick the subset of `package.json` fields that downstream code
/// (bin linking, dependency resolution, build-script detection)
/// actually reads, and discard the rest. Two reasons for the
/// subset: (1) `index.db` row size on disk — a full manifest can be
/// tens of KB; (2) msgpackr-records slot pressure on the encoder
/// (record slots top out at `0x7f`, see [`pacquet_store_dir::EncodeError::OutOfRecordSlots`]).
///
/// `scripts` is further narrowed to just the three lifecycle hooks
/// pnpm actually executes — `preinstall`, `install`, `postinstall`.
/// Other script keys (test, build, lint, etc.) are dev ergonomics
/// that the installer never invokes.
///
/// Returns `None` when nothing survives the pick. In practice this
/// only happens for inputs that aren't a JSON object at all (the
/// type guard at the top of the function), or for objects whose
/// every field is either absent or `null`. A real `package.json`
/// from an npm tarball always carries at least `name` and `version`
/// (both kept by the pick), so the typical npm-published manifest
/// surfaces as `Some(...)`. Empty inputs degrade to `None` rather
/// than a `Some(Object({}))` that would round-trip as a zero-field
/// record def.
pub(crate) fn normalize_bundled_manifest(value: &serde_json::Value) -> Option<serde_json::Value> {
    /// Fields kept verbatim from the source manifest.
    ///
    /// Order matters for the on-wire byte sequence — msgpackr emits
    /// fields in JS object insertion order, and pacquet's encoder
    /// follows the [`serde_json::Map`] iteration order — but it
    /// does *not* matter for property-access correctness on the
    /// pnpm side. The order below matches the field order pnpm
    /// emits so a side-by-side byte diff against a pnpm-written
    /// row is shallower.
    const BUNDLED_MANIFEST_FIELDS: &[&str] = &[
        "bin",
        "bundledDependencies",
        "bundleDependencies",
        "cpu",
        "dependencies",
        "devDependencies",
        "directories",
        "engines",
        "libc",
        "name",
        "optionalDependencies",
        "os",
        "peerDependencies",
        "peerDependenciesMeta",
    ];
    const LIFECYCLE_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

    let serde_json::Value::Object(map) = value else { return None };
    let mut picked = serde_json::Map::new();

    // pnpm emits `version` first regardless of whether it was first
    // in the source object. Keep the same ordering so a byte diff
    // against a pnpm-written row stays minimal. Version normalization
    // via `semver.clean(...)` (pnpm only loose-cleans for the bundled
    // row, not for resolution) is intentionally skipped: the inputs
    // from a real npm tarball are already semver-clean in practice,
    // and pulling `node-semver` into `pacquet-tarball` purely for
    // this normalization would carry more risk than the deviation it
    // closes.
    if let Some(v) = map.get("version")
        && !v.is_null()
    {
        picked.insert("version".to_string(), v.clone());
    }

    for &key in BUNDLED_MANIFEST_FIELDS {
        if let Some(v) = map.get(key)
            && !v.is_null()
        {
            picked.insert(key.to_string(), v.clone());
        }
    }

    if let Some(serde_json::Value::Object(scripts)) = map.get("scripts") {
        let mut sub = serde_json::Map::new();
        for &key in LIFECYCLE_SCRIPTS {
            if let Some(s) = scripts.get(key)
                && !s.is_null()
            {
                sub.insert(key.to_string(), s.clone());
            }
        }
        if !sub.is_empty() {
            picked.insert("scripts".to_string(), serde_json::Value::Object(sub));
        }
    }

    if picked.is_empty() { None } else { Some(serde_json::Value::Object(picked)) }
}

/// One regular-file tar entry whose path has been validated and
/// cleaned, paired with a borrow of its payload inside the decompressed
/// archive buffer. Collected serially while walking the tar stream, then
/// hashed and written to the CAFS — serially or across the rayon pool —
/// in [`write_cas_entry`].
pub(crate) struct PendingFile<'a> {
    cleaned_path: String,
    data: &'a [u8],
    executable: bool,
    mode: u32,
    size: u64,
}

/// Hash one [`PendingFile`] into the content-addressed store and build
/// its [`CafsFileInfo`] index row. Pure given the inputs and the store
/// dir's content-addressed layout, so it is safe to run concurrently
/// across entries of the same tarball.
pub(crate) fn write_cas_entry(
    store_dir: &StoreDir,
    file: &PendingFile<'_>,
) -> Result<(String, PathBuf, CafsFileInfo), TarballError> {
    let (file_path, file_hash) =
        store_dir.write_cas_file(file.data, file.executable).map_err(TarballError::WriteCasFile)?;
    // `as_millis()` returns `u128`; narrow to `u64` to match the store
    // index schema (see `CafsFileInfo::checked_at`). Drop the timestamp
    // if the clock reports something unrepresentable — `checkedAt` is
    // optional and pnpm tolerates `None`.
    let checked_at =
        UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    let info = CafsFileInfo {
        digest: format!("{file_hash:x}"),
        mode: file.mode,
        size: file.size,
        checked_at,
    };
    Ok((file.cleaned_path.clone(), file_path, info))
}

/// Fold a synthesized `package.json` (pnpm's `appendManifest`) into a
/// freshly extracted archive's CAFS output. Runtime archives (Node.js /
/// Bun / Deno) carry no `package.json` of their own, so the caller
/// supplies one, and it also becomes the store-index row's bundled
/// `manifest` — which is what lets the warm-batch bin linker find the
/// runtime's bin without a disk round-trip.
///
/// See [`write_synthesized_package_json`] for what reaching the store
/// entails and when the write is skipped.
pub(crate) fn apply_append_manifest(
    store_dir: &StoreDir,
    manifest_bytes: &[u8],
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<(), TarballError> {
    if !write_synthesized_package_json(store_dir, manifest_bytes, cas_paths, pkg_files_idx)? {
        return Ok(());
    }
    // Surface the synthesized manifest as the row's bundled manifest so
    // the warm-batch bin linker reads the bin here instead of stat-ing the
    // slot. Only when the archive supplied none, mirroring pnpm's guard.
    if pkg_files_idx.manifest.is_none()
        && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(manifest_bytes)
    {
        pkg_files_idx.manifest = normalize_bundled_manifest(&parsed);
    }
    Ok(())
}

/// Give an archive that ships no `package.json` of its own the
/// placeholder one pnpm writes, so every extracted package has one and
/// materialization can treat it as the slot's completion marker.
///
/// The placeholder is a marker, not a manifest: its `_pnpmPlaceholder`
/// field is how a reader tells it apart from a real one, and the
/// store-index row's bundled `manifest` stays empty so nothing mistakes
/// it for the package's identity.
///
/// See [`write_synthesized_package_json`] for what reaching the store
/// entails and when the write is skipped — a real `package.json`,
/// including one [`apply_append_manifest`] just synthesized, always
/// takes precedence.
pub(crate) fn apply_placeholder_manifest(
    store_dir: &StoreDir,
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<(), TarballError> {
    write_synthesized_package_json(store_dir, PLACEHOLDER_PACKAGE_JSON, cas_paths, pkg_files_idx)?;
    Ok(())
}

/// The `package.json` pnpm writes for a package that genuinely has none.
/// The `_pnpmPlaceholder` field tells a manifest reader to ignore it.
pub(crate) const PLACEHOLDER_PACKAGE_JSON: &[u8] = br#"{"_pnpmPlaceholder":"This file was generated by pnpm. The original package did not contain a package.json."}"#;

/// Write `bytes` into the content-addressed store as the archive's
/// `package.json`, recording it in both `cas_paths` (this install's
/// slot) and the persisted `pkg_files_idx`. Baking the file into the
/// store-index row is what lets a later warm materialization land a
/// `package.json` slot without re-extracting.
///
/// Returns whether anything was written — `false` when the archive
/// already carries a `package.json`, which always wins.
pub(crate) fn write_synthesized_package_json(
    store_dir: &StoreDir,
    bytes: &[u8],
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<bool, TarballError> {
    if pkg_files_idx.files.contains_key("package.json") {
        return Ok(false);
    }
    let (cas_path, file_hash) =
        store_dir.write_cas_file(bytes, false).map_err(TarballError::WriteCasFile)?;
    let checked_at =
        UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    let info = CafsFileInfo {
        digest: format!("{file_hash:x}"),
        // A synthesized manifest is a plain, non-executable data file;
        // `0o644` is the same canonical mode `add_files_from_dir` reports
        // for a non-executable entry (and pnpm's Windows-host default).
        mode: 0o644,
        size: bytes.len() as u64,
        checked_at,
    };
    cas_paths.insert("package.json".to_string(), cas_path);
    pkg_files_idx.files.insert("package.json".to_string(), info);
    Ok(true)
}

/// Walk decompressed tar bytes, writing each regular-file entry into
/// the CAFS and returning the `{in-tarball path → CAFS path}` map plus
/// the per-tarball [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer.
///
/// Non-regular-file entries (symlinks, hardlinks, character / block
/// devices, fifos, GNU / PAX extension headers, directories) are
/// filtered out. Real npm-publish tarballs only carry regular files;
/// anything else would need custom handling that pacquet doesn't yet
/// do, and silently reading a symlink's 0-byte body into the CAFS as
/// if it were a file would just corrupt the store.
///
/// The archive is already fully buffered in memory by the download
/// pipeline. Use `entries_with_seek` + `raw_file_position` to borrow
/// each file payload as a slice of that buffer instead of allocating a
/// fresh `Vec<u8>` and `read_to_end`-ing every entry.
///
/// Every tar-side failure comes back as
/// [`TarballError::ReadTarballEntries`] instead of panicking.
/// Non-UTF-8 entry paths are coerced via
/// [`std::path::Path::to_string_lossy`], matching pnpm's string-based
/// handling so a mixed install against the shared `index.db` stays
/// consistent; real-world npm tarballs are UTF-8 so the coercion is
/// almost never hit in practice.
pub(crate) fn extract_tarball_entries(
    tar_data: &[u8],
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let mut archive = Archive::new(Cursor::new(tar_data));
    let entries = archive
        .entries_with_seek()
        .map_err(TarballError::ReadTarballEntries)?
        // Keep only regular-file `Ok` entries; anything else in the
        // `Ok` arm (directories, symlinks, hardlinks, pax/gnu
        // extension headers, ...) is dropped. `Err` entries fall
        // through so the `?` inside the loop below propagates them.
        .filter(|entry| match entry {
            Ok(entry) => entry.header().entry_type().is_file(),
            Err(_) => true,
        });

    let ((_, Some(capacity)) | (capacity, None)) = entries.size_hint();

    // Phase 1 (serial): walk the seekable tar stream, validate and clean
    // each regular-file path, and capture the byte slice of its payload.
    // Header parsing has to run sequentially against the single archive
    // stream, but it's cheap; the expensive per-file hashing + CAS write
    // is deferred to the parallel phase below. The bundled `package.json`
    // manifest is captured here too, off the raw payload slice.
    let mut pending: Vec<PendingFile<'_>> = Vec::with_capacity(capacity);
    let mut manifest = None;
    let mut manifest_build_scripts = false;
    let mut file_build_hooks = false;

    for entry in entries {
        let entry = entry.map_err(TarballError::ReadTarballEntries)?;

        let file_mode = entry.header().mode().map_err(TarballError::ReadTarballEntries)?;
        let file_is_executable = file_mode::is_executable(file_mode);
        let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
        let entry_data = tar_entry_payload(tar_data, &entry)?;

        let entry_path = entry.path().map_err(TarballError::ReadTarballEntries)?;
        // `components().skip(1)` drops the top-level package
        // directory (`package/`). Every remaining component must be
        // `Component::Normal`: a hostile tarball can carry `..`,
        // absolute-root, or Windows-prefix components that — joined
        // onto the CAFS extraction root later in `create_cas_files`
        // — would land files outside the store (directory traversal).
        // Reject loudly rather than silently normalize so tampering
        // is visible.
        //
        // Collect components into a `Vec<String>` and join with `/`
        // rather than going through [`PathBuf::push`] + `to_string_lossy`.
        // `PathBuf` uses the platform's native separator, so on
        // Windows the joined form would be `bin\tool` — which
        // diverges from pnpm's string-based path layer (always `/`)
        // and breaks any [`ignore_file_pattern`] regex / hand-coded
        // matcher that expects forward slashes. The shared `index.db`
        // also has to stay byte-identical to what pnpm writes, so a
        // pacquet install on Windows must emit the same keys.
        // `to_string_lossy()` coerces non-UTF-8 bytes to U+FFFD
        // per-component.
        let mut parts: Vec<String> = Vec::new();
        for component in entry_path.components().skip(1) {
            let Component::Normal(part) = component else {
                return Err(TarballError::ReadTarballEntries(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "tar entry path rejected (non-normal component, possible directory traversal): {entry_path:?}",
                    ),
                )));
            };
            parts.push(part.to_string_lossy().into_owned());
        }
        if parts.is_empty() {
            return Err(TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "tar entry path has no payload after dropping the top-level component: {entry_path:?}",
                ),
            )));
        }
        let cleaned_entry_path = parts.join("/");
        // Drop ignored entries before the CAS write. Paths are matched
        // *after* the top-level prefix strip, so the callback sees the
        // cleaned relative path. Bypassing the CAS write here also
        // keeps the package's [`PackageFilesIndex`] tight — an ignored
        // entry never surfaces in `files` or `manifest`.
        if let Some(filter) = ignore_file_pattern
            && filter(&cleaned_entry_path)
        {
            continue;
        }
        if files_include_install_scripts([cleaned_entry_path.as_str()]) {
            file_build_hooks = true;
        }
        // Capture the parsed manifest whenever we see `package.json`.
        // The narrowed manifest is stashed in `pkgFilesIndex.manifest`
        // so install-side consumers (notably bin linking) can avoid
        // re-reading the file from disk — the same place pnpm keeps it,
        // so the shared `index.db` row carries it for both tools. The
        // [`normalize_bundled_manifest`] pick drops fields downstream
        // code doesn't use, keeping `index.db` rows tight.
        //
        // **Last-entry wins.** A duplicate `package.json` entry
        // overwrites any earlier one, so the final entry is canonical
        // — same shape as the `files` map, which already overwrites
        // duplicates. Real npm tarballs never publish multiple
        // `package.json` entries, but the consistency with the `files`
        // map is what matters: `manifest` and `files` must describe the
        // same file. Failed JSON parses degrade the field to `None` (the
        // manifest is best-effort; a corrupt `package.json` is the
        // publisher's fault and downstream code can fall back to
        // disk reads).
        if cleaned_entry_path == "package.json" {
            match parse_manifest_bytes(entry_data) {
                Ok(parsed) => {
                    manifest_build_scripts = manifest_requires_build(&parsed);
                    manifest = normalize_bundled_manifest(&parsed);
                }
                Err(error) => {
                    tracing::debug!(
                        ?error,
                        "package.json in tarball failed to parse as JSON; bundled manifest cleared",
                    );
                    manifest_build_scripts = false;
                    manifest = None;
                }
            }
        }

        pending.push(PendingFile {
            cleaned_path: cleaned_entry_path,
            data: entry_data,
            executable: file_is_executable,
            mode: file_mode,
            size: file_size,
        });
    }

    // Phase 2: hash and write every file into the content-addressed
    // store. Extracting a package with thousands of files (e.g.
    // `core-js`) on a single blocking thread pins one core while the
    // rest sit idle — most costly at the makespan tail, when it's the
    // last extraction still running. `write_cas_entry` is safe to run
    // concurrently, so large tarballs fan out across the dedicated
    // [`cas_write_pool`]; small ones stay serial to skip rayon's per-job
    // dispatch cost when there's nothing to gain. The dedicated pool
    // keeps this off the global pool the linker uses, so an extraction
    // burst can't stall node_modules linking running concurrently.
    const PARALLEL_EXTRACT_THRESHOLD: usize = 32;
    let written: Vec<(String, PathBuf, CafsFileInfo)> =
        if pending.len() >= PARALLEL_EXTRACT_THRESHOLD {
            let write_all = || -> Result<Vec<(String, PathBuf, CafsFileInfo)>, TarballError> {
                pending.par_iter().map(|file| write_cas_entry(store_dir, file)).collect()
            };
            match cas_write_pool() {
                Some(pool) => pool.install(write_all),
                None => write_all(),
            }?
        } else {
            pending.iter().map(|file| write_cas_entry(store_dir, file)).collect::<Result<_, _>>()?
        };

    // Phase 3 (serial): assemble the output maps. `written` preserves
    // `pending` order, so a tarball with duplicate paths keeps the last
    // entry — matching pnpm's last-wins `filesIndex.set`.
    let mut cas_paths = HashMap::<String, PathBuf>::with_capacity(written.len());
    let mut files = HashMap::with_capacity(written.len());
    for (path, file_path, info) in written {
        if let Some(previous) = cas_paths.insert(path.clone(), file_path) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
        if let Some(previous) = files.insert(path, info) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
    }

    let pkg_files_idx = PackageFilesIndex {
        manifest,
        requires_build: Some(manifest_build_scripts || file_build_hooks),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    Ok((cas_paths, pkg_files_idx))
}

/// Borrow one tar entry's payload out of the decompressed archive.
///
/// The tar reader is seekable over an in-memory buffer, so an entry's
/// bytes are already there — slicing them costs nothing, where reading
/// through the entry would copy every payload into a fresh allocation
/// sized by the archive's own (untrusted) header.
///
/// The bounds are all checked: a header whose offset or size doesn't fit
/// a `usize`, whose sum overflows, or whose range runs past the end of
/// the archive is rejected rather than truncated.
pub(crate) fn tar_entry_payload<'a, Reader: std::io::Read>(
    tar_data: &'a [u8],
    entry: &tar::Entry<'_, Reader>,
) -> Result<&'a [u8], TarballError> {
    let invalid = |message: &str| {
        TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message.to_string(),
        ))
    };
    let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
    let data_offset = usize::try_from(entry.raw_file_position())
        .map_err(|_| invalid("tar entry file offset does not fit in usize"))?;
    let size = usize::try_from(file_size)
        .map_err(|_| invalid("tar entry file size does not fit in usize"))?;
    let end = data_offset
        .checked_add(size)
        .ok_or_else(|| invalid("tar entry file offset plus size overflows usize"))?;
    tar_data.get(data_offset..end).ok_or_else(|| {
        TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "tar entry payload extends beyond archive",
        ))
    })
}

use super::{
    CafsFileInfo, HashMap, PackageFilesIndex, PathBuf, StoreDir, TarballError, UNIX_EPOCH,
    manifest_requires_build, parse_manifest_bytes,
};

/// Pick the `package.json` fields downstream code actually reads — bin
/// linking, dependency resolution, build-script detection — and discard
/// the rest, keeping only the three lifecycle hooks pnpm executes out
/// of `scripts`.
///
/// The subset exists to bound what lands in `index.db`: a full manifest
/// runs to tens of KB, and msgpackr-records tops out at `0x7f` record
/// slots (see [`pnpm_store_dir::EncodeError::OutOfRecordSlots`]).
///
/// `None` rather than an empty object when nothing survives, which
/// would otherwise round-trip as a zero-field record def.
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
    // and pulling `node-semver` into `pnpm-tarball` purely for
    // this normalization would carry more risk than the deviation it
    // closes.
    pick_fields(&mut picked, map, &["version"]);
    pick_fields(&mut picked, map, BUNDLED_MANIFEST_FIELDS);

    if let Some(serde_json::Value::Object(scripts)) = map.get("scripts") {
        let mut sub = serde_json::Map::new();
        pick_fields(&mut sub, scripts, LIFECYCLE_SCRIPTS);
        if !sub.is_empty() {
            picked.insert("scripts".to_string(), serde_json::Value::Object(sub));
        }
    }

    if picked.is_empty() { None } else { Some(serde_json::Value::Object(picked)) }
}

/// Copy the named fields that `source` actually carries, in the order given.
/// A null is treated as absent: pnpm's own row omits it.
fn pick_fields(
    picked: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) {
    for &key in keys {
        if let Some(value) = source.get(key)
            && !value.is_null()
        {
            picked.insert(key.to_string(), value.clone());
        }
    }
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

/// Parse a tarball's bundled `package.json`, returning its
/// requires-build flag and the narrowed manifest for the store-index
/// row.
///
/// The narrowed manifest is stashed in `pkgFilesIndex.manifest` so
/// install-side consumers (notably bin linking) can avoid re-reading
/// the file from disk — the same place pnpm keeps it, so the shared
/// `index.db` row carries it for both tools. The
/// [`normalize_bundled_manifest`] pick drops fields downstream code
/// doesn't use, keeping `index.db` rows tight.
///
/// Callers apply this to every `package.json` entry they see, so a
/// duplicate entry overwrites any earlier one and the final entry is
/// canonical — same shape as the `files` map, which already overwrites
/// duplicates. Real npm tarballs never publish multiple `package.json`
/// entries, but the consistency with the `files` map is what matters:
/// `manifest` and `files` must describe the same file.
///
/// Failed JSON parses degrade to `(false, None)` — the manifest is
/// best-effort; a corrupt `package.json` is the publisher's fault and
/// downstream code can fall back to disk reads.
pub(super) fn capture_bundled_manifest(entry_data: &[u8]) -> (bool, Option<serde_json::Value>) {
    match parse_manifest_bytes(entry_data) {
        Ok(parsed) => (manifest_requires_build(&parsed), normalize_bundled_manifest(&parsed)),
        Err(error) => {
            tracing::debug!(
                ?error,
                "package.json in tarball failed to parse as JSON; bundled manifest cleared",
            );
            (false, None)
        }
    }
}

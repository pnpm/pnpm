use std::collections::{BTreeMap, BTreeSet};

use pnpm_package_manifest::{
    PackageBin, ScriptRuntime, manifest_declares_bins, parse_shebang_from_bytes,
    resolve_package_bins_with,
};

use super::{
    BuildTriggers, CafsFileInfo, HashMap, PackageFilesIndex, PathBuf, StoreDir, TarballError,
    UNIX_EPOCH, parse_manifest_bytes,
};

/// Reads a TAR entry that must fit in memory, rejecting oversized payloads
/// before allocating or reading them.
pub fn read_buffered_tar_entry(
    entry: &mut tar::Entry<'_, impl std::io::Read>,
) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let size = entry.size();
    if size > crate::MAX_TARBALL_METADATA_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "tar entry {} is {size} bytes, which exceeds the {}-byte buffered entry limit",
                entry
                    .path()?
                    .to_string_lossy()
                    .escape_debug(),
                crate::MAX_TARBALL_METADATA_BYTES,
            ),
        ));
    }
    let mut data = Vec::with_capacity(size as usize);
    entry.read_to_end(&mut data)?;
    Ok(data)
}

/// Pick the `package.json` fields downstream code actually reads — bin
/// linking, dependency resolution, build-script detection — and discard
/// the rest, keeping only the lifecycle hooks pnpm executes out
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
        "gypfile",
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
    // in the source object. Version normalization
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
    let checked_at = UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
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

/// Parse a tarball's bundled `package.json`, recording its build triggers in
/// `triggers` and returning the narrowed manifest for the store-index row.
///
/// The narrowed manifest is stashed in `pkgFilesIndex.manifest` so
/// install-side consumers (notably bin linking) can avoid re-reading
/// the file from disk — the same place pnpm keeps it, so the shared
/// `index.db` row carries it for both tools.
///
/// Callers apply this to every `package.json` entry they see, so a
/// duplicate entry overwrites any earlier one and the final entry is
/// canonical — same shape as the `files` map, which already overwrites
/// duplicates. Real npm tarballs never publish multiple `package.json`
/// entries, but the consistency with the `files` map is what matters:
/// `manifest` and `files` must describe the same file.
///
/// A failed JSON parse leaves `triggers` untouched and returns no manifest —
/// the manifest is best-effort; a corrupt `package.json` is the publisher's
/// fault and downstream code can fall back to disk reads.
pub(super) fn capture_bundled_manifest(
    entry_data: &[u8],
    triggers: &mut BuildTriggers,
) -> Option<serde_json::Value> {
    match parse_manifest_bytes(entry_data) {
        Ok(parsed) => {
            triggers.read_manifest(&parsed);
            normalize_bundled_manifest(&parsed)
        }
        Err(error) => {
            tracing::debug!(
                ?error,
                "package.json in tarball failed to parse as JSON; bundled manifest cleared",
            );
            None
        }
    }
}

/// Ceiling on PAX / GNU extension headers when scanning a tar stream for
/// build triggers.
const TAR_SCAN_MAX_METADATA_BYTES: u64 = 64 * 1024;

/// Ceiling on the `package.json` entry buffered during [`tar_requires_build`].
const TAR_SCAN_MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// Maximum number of regular-file paths retained while precalculating bins in
/// [`inspect_tar`]. Archives above this cap skip bin precalculation so
/// concurrent tar scans stay memory-bounded.
const TAR_SCAN_MAX_BIN_FILES: usize = 16_384;

/// Facts derived from a single scan of a `.tar` stream.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TarInspection {
    /// Whether the archive contains lifecycle scripts, `binding.gyp`
    /// (unless `gypfile: false`), or `.hooks/`. `None` when the tar stream
    /// cannot be read, an entry path fails validation, or `package.json`
    /// fails to parse as JSON.
    pub requires_build: Option<bool>,
    /// Number of regular files in the archive.
    pub file_count: Option<u64>,
    /// Total unpacked byte size of regular files in the archive.
    pub unpacked_size: Option<u64>,
    /// Precalculated bin commands and shebang/extension runtimes for the
    /// archive. Empty when the package declares no bins or when a declared bin
    /// target must be resolved on disk at link time.
    pub bins: Option<BTreeMap<String, PackageBin>>,
}

/// Inspect a `.tar` stream in a single pass, recording regular-file counts
/// and unpacked byte totals alongside the [`tar_requires_build`] verdict.
pub fn inspect_tar(reader: impl std::io::Read) -> TarInspection {
    inspect_tar_entries(reader).unwrap_or_default()
}

struct TarStreamReader<Reader> {
    inner: Reader,
    unexpected_eof: bool,
}

impl<Reader: std::io::Read> std::io::Read for TarStreamReader<Reader> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.inner.read(buf)?;
        if len == 0 && !buf.is_empty() {
            self.unexpected_eof = true;
        }
        Ok(len)
    }
}

#[derive(Default)]
struct TarBinScan {
    archive_files: BTreeSet<String>,
    shebangs: HashMap<String, ScriptRuntime>,
    skipped: bool,
    overflowed: bool,
}

impl TarBinScan {
    fn on_manifest(&mut self, manifest: Option<&serde_json::Value>, requires_build: bool) {
        if self.skipped {
            self.overflowed = true;
        }
        if manifest.is_some_and(manifest_declares_bins) && !requires_build {
            self.record("package.json".to_string(), None);
        } else {
            self.skip();
        }
    }

    fn on_file<Reader: std::io::Read>(
        &mut self,
        cleaned_path: String,
        entry: &mut tar::Entry<'_, Reader>,
        requires_build: bool,
    ) -> Option<()> {
        if requires_build {
            self.skip();
        } else if !self.skipped {
            let shebang = read_tar_entry_shebang(entry)?;
            self.record(cleaned_path, shebang);
        }
        Some(())
    }

    fn record(&mut self, cleaned_path: String, shebang: Option<ScriptRuntime>) {
        if self.archive_files.len() >= TAR_SCAN_MAX_BIN_FILES
            && !self.archive_files.contains(&cleaned_path)
        {
            self.overflowed = true;
            self.skip();
            return;
        }
        if let Some(runtime) = shebang {
            self.shebangs.insert(cleaned_path.clone(), runtime);
        } else {
            self.shebangs.remove(&cleaned_path);
        }
        self.archive_files.insert(cleaned_path);
    }

    fn skip(&mut self) {
        self.skipped = true;
        self.archive_files.clear();
        self.shebangs.clear();
    }

    fn finish(self, manifest: Option<&serde_json::Value>) -> BTreeMap<String, PackageBin> {
        if self.overflowed {
            return BTreeMap::new();
        }
        manifest
            .and_then(|parsed| {
                resolve_package_bins_with(
                    parsed,
                    |path| self.archive_files.contains(path),
                    self.archive_files.iter().map(String::as_str),
                    |path| self.shebangs.get(path).cloned(),
                )
            })
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct TarScanState {
    triggers: BuildTriggers,
    manifest: Option<serde_json::Value>,
    manifest_parse_failed: bool,
    file_count: u64,
    unpacked_size: u64,
    bins: TarBinScan,
}

impl TarScanState {
    fn visit_entry<Reader: std::io::Read>(
        &mut self,
        entry: &mut tar::Entry<'_, Reader>,
    ) -> Option<()> {
        self.file_count = self.file_count.checked_add(1)?;
        self.unpacked_size = self.unpacked_size.checked_add(entry.size())?;
        let raw_path = entry.path().ok()?;
        let cleaned_path = super::clean_archive_entry_path(&raw_path.to_string_lossy()).ok()?;
        self.triggers.add_file(&cleaned_path);
        if cleaned_path == "package.json" {
            self.manifest = read_tar_manifest_entry(entry, &mut self.triggers)?;
            self.manifest_parse_failed = self.manifest.is_none();
            self.bins.on_manifest(self.manifest.as_ref(), self.triggers.requires_build());
            Some(())
        } else {
            let requires_build = self.manifest.is_some() && self.triggers.requires_build();
            self.bins.on_file(cleaned_path, entry, requires_build)
        }
    }

    fn finish(self) -> TarInspection {
        let requires_build = (!self.manifest_parse_failed).then(|| self.triggers.requires_build());
        let bins =
            (requires_build == Some(false)).then(|| self.bins.finish(self.manifest.as_ref()));
        TarInspection {
            requires_build,
            file_count: Some(self.file_count),
            unpacked_size: Some(self.unpacked_size),
            bins,
        }
    }
}

fn inspect_tar_entries(reader: impl std::io::Read) -> Option<TarInspection> {
    let mut archive = tar::Archive::new(TarStreamReader { inner: reader, unexpected_eof: false });
    archive.set_max_metadata_size(Some(TAR_SCAN_MAX_METADATA_BYTES));
    let mut state = TarScanState::default();
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        if entry.header().entry_type().is_file() {
            state.visit_entry(&mut entry)?;
        }
    }
    (!archive.into_inner().unexpected_eof).then(|| state.finish())
}

/// Whether the tar stream read from `reader` carries build triggers:
/// lifecycle scripts (`preinstall`, `install`, `postinstall`) in its
/// root `package.json`, a `binding.gyp` (unless `gypfile: false` in
/// `package.json`), or a `.hooks/` entry.
///
/// Returns `None` when the tar stream cannot be read or an entry path fails
/// validation.
pub fn tar_requires_build(reader: impl std::io::Read) -> Option<bool> {
    inspect_tar(reader).requires_build
}

fn read_tar_manifest_entry<Reader: std::io::Read>(
    entry: &mut tar::Entry<'_, Reader>,
    triggers: &mut BuildTriggers,
) -> Option<Option<serde_json::Value>> {
    use std::io::Read as _;

    let size = entry.size();
    if size > TAR_SCAN_MAX_MANIFEST_BYTES {
        return None;
    }
    let capacity = usize::try_from(size).ok()?;
    let mut data = Vec::new();
    data.try_reserve_exact(capacity).ok()?;
    entry.read_to_end(&mut data).ok()?;
    if data.len() as u64 != size {
        return None;
    }
    let Ok(parsed) = parse_manifest_bytes(&data) else {
        triggers.forget_manifest();
        return Some(None);
    };
    triggers.read_manifest(&parsed);
    Some(Some(parsed))
}

fn read_tar_entry_shebang<Reader: std::io::Read>(
    entry: &mut tar::Entry<'_, Reader>,
) -> Option<Option<ScriptRuntime>> {
    let mut head = [0u8; 512];
    let filled = super::streaming::read_prefix(entry, &mut head).ok()?;
    Some(parse_shebang_from_bytes(&head[..filled]))
}

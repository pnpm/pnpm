use super::InstallPackageBySnapshotError;
use pnpm_lockfile::{BinaryArchive, BinaryResolution, BinarySpec, PackageKey};
use pnpm_reporter::Reporter;
use pnpm_tarball::{IgnoreEntryFilter, IngestTarballToStore, IngestZipArchiveToStore};
use std::sync::Arc;

/// Hand-coded matcher for the
/// `^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)`
/// regex. Used as the archive-entry filter when extracting a Node.js
/// runtime archive: pnpm bundles `npm` + `corepack` in the tarball,
/// but pacquet (and pnpm) install pnpm itself as the package
/// manager, so the bundled tooling is dead weight and would also
/// shadow the user's pnpm via `node_modules/.bin/`. Stripping these
/// entries during the CAS write keeps the runtime artifact in the
/// store free of the bundled tooling without a post-hoc cleanup.
///
/// The hand-coded matcher avoids pulling a regex engine into
/// [`pnpm_tarball`].
pub(super) fn node_extras_filter(path: &str) -> bool {
    bundled_tooling_module(path) || bundled_tooling_bin(path) || bundled_tooling_root_entry(path)
}
/// `^(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)`
pub(super) fn bundled_tooling_module(path: &str) -> bool {
    let after_lib = path.strip_prefix("lib/").unwrap_or(path);
    let Some(rest) = after_lib.strip_prefix("node_modules/") else {
        return false;
    };
    ["npm", "corepack"]
        .into_iter()
        .any(|name| {
            rest.strip_prefix(name)
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
        })
}
/// `^bin/(?:npm|npx|corepack)$`
pub(super) fn bundled_tooling_bin(path: &str) -> bool {
    path.strip_prefix("bin/")
        .is_some_and(|rest| matches!(rest, "npm" | "npx" | "corepack"))
}
/// `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
///
/// These are *not* under `bin/` — they live at the runtime archive root
/// after the `node-vX.Y.Z-<platform>-<arch>/` prefix strip.
pub(super) fn bundled_tooling_root_entry(path: &str) -> bool {
    let stem = path
        .strip_suffix(".cmd")
        .or_else(|| path.strip_suffix(".ps1"))
        .unwrap_or(path);
    matches!(stem, "npm" | "npx" | "corepack")
}
/// Build the per-fetch [`IgnoreEntryFilter`] for the package being
/// installed.
///
/// The filter is cached in a [`std::sync::LazyLock`] so per-snapshot
/// `Arc::clone`s share one trait object — `IgnoreEntryFilter` is
/// a `dyn Fn`, so cheap to clone, and we don't want to allocate
/// the Arc once per runtime install.
pub(crate) fn archive_filter_for(package_key: &PackageKey) -> Option<Arc<IgnoreEntryFilter>> {
    if package_key.name.scope.is_some() || package_key.name.bare != "node" {
        return None;
    }
    static FILTER: std::sync::LazyLock<Arc<IgnoreEntryFilter>> = std::sync::LazyLock::new(|| {
        // `fn(&str) -> bool` implements `Fn(&str) -> bool + Send +
        // Sync`, so an `Arc<fn(...)>` unsizes to
        // `Arc<dyn Fn(...) + Send + Sync>` (the trait-object type
        // `IgnoreEntryFilter` aliases). The explicit type
        // annotation drives the unsizing coercion.
        let inner: Arc<IgnoreEntryFilter> = Arc::new(node_extras_filter);
        inner
    });
    Some(Arc::clone(&FILTER))
}
/// Fetch a [`BinaryResolution`] into the CAS, returning the
/// per-file `{relative_path → cas_path}` map the snapshot's virtual
/// directory needs. Dispatches on the archive type:
///
/// - [`BinaryArchive::Tarball`] uses [`IngestTarballToStore`]
///   with `package_unpacked_size: None` (binary archives don't
///   carry that hint).
/// - [`BinaryArchive::Zip`] uses [`IngestZipArchiveToStore`]
///   with `archive_prefix: binary.prefix.as_deref()` so the runtime
///   archive's top-level wrapper (e.g.
///   `node-v22.0.0-darwin-arm64/`) is stripped before the CAS keys
///   are written.
pub(crate) async fn fetch_binary_resolution_to_cas<Reporter: self::Reporter>(
    binary: &BinaryResolution,
    download: &IngestTarballToStore<'_>,
    package_key: &PackageKey,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<pnpm_fs::CasPaths, InstallPackageBySnapshotError> {
    let package_id = package_key.pkg_id();

    // Synthesize the `package.json` runtime archives (Node.js / Bun /
    // Deno) don't ship, and hand it to the fetcher as `append_manifest`.
    // The fetcher folds it into both this install's `cas_paths` and the
    // persisted store-index row (its `files` map and bundled `manifest`),
    // so a later *warm* install — which materializes straight from the
    // row and never re-runs this function — still lands a `package.json`
    // slot and lets the bin linker find the runtime's bin. The object
    // carries `name`, `version`, and `bin` — the three fields pacquet's
    // bin linking and `dlx` look at.
    let manifest_bytes = synthesize_runtime_manifest_bytes(package_key, binary)?;
    let fetch = BinaryArchiveFetch {
        binary,
        download,
        package_id: &package_id,
        ignore_file_pattern,
        manifest_bytes: &manifest_bytes,
    };
    match binary.archive {
        BinaryArchive::Tarball => fetch.tarball::<Reporter>().await,
        BinaryArchive::Zip => fetch.zip::<Reporter>().await,
    }
}
pub(super) struct BinaryArchiveFetch<'a> {
    binary: &'a BinaryResolution,
    download: &'a IngestTarballToStore<'a>,
    package_id: &'a str,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    manifest_bytes: &'a [u8],
}
impl BinaryArchiveFetch<'_> {
    async fn tarball<Reporter: self::Reporter>(
        self,
    ) -> Result<pnpm_fs::CasPaths, InstallPackageBySnapshotError> {
        let download = self.download;
        IngestTarballToStore {
            fetching: download.fetching,
            package: pnpm_tarball::TarballPackage {
                integrity_kind: pnpm_tarball::IntegrityKind::Resource,
                integrity: Some(&self.binary.integrity),
                unpacked_size: None,
                file_count: None,
                url: &self.binary.url,
                id: self.package_id,
            },
            store: download.store.clone(),

            requester: download.requester,

            ignore_file_pattern: self.ignore_file_pattern,

            // Cold-batch self.binary tarball download: emits `fetched`
            // directly, so no network-fetched tracking is needed.
            progress_reported: None,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(self.manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
    }
    async fn zip<Reporter: self::Reporter>(
        self,
    ) -> Result<pnpm_fs::CasPaths, InstallPackageBySnapshotError> {
        let download = self.download;
        IngestZipArchiveToStore {
            fetching: download.fetching,
            package: pnpm_tarball::ZipArchivePackage {
                max_bytes: None,
                integrity: &self.binary.integrity,
                url: &self.binary.url,
                id: self.package_id,
            },
            store: download.store.clone(),

            requester: download.requester,

            archive_prefix: self.binary.prefix.as_deref(),
            ignore_file_pattern: self.ignore_file_pattern,

            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(self.manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
    }
}
/// The store index key of the row that a runtime install writes for
/// `binary`, which includes its synthesized manifest.
pub fn binary_store_index_key(
    package_key: &PackageKey,
    binary: &BinaryResolution,
) -> Result<String, InstallPackageBySnapshotError> {
    let manifest = synthesize_runtime_manifest_bytes(package_key, binary)?;
    Ok(pnpm_tarball::ArchiveStoreProjection::Package { append_manifest: Some(&manifest) }
        .store_index_key(&binary.integrity.to_string(), &package_key.pkg_id()))
}
/// Serialize the synthesized runtime `package.json` to bytes.
///
/// `serde_json::to_vec` writes a single-line UTF-8 blob. The bytes go
/// straight into the CAS, where they're addressed by the SHA-512 of their
/// content; two runtime archives whose `(name, version, bin)`
/// triple happens to match share the same blob.
pub(super) fn synthesize_runtime_manifest_bytes(
    package_key: &PackageKey,
    binary: &BinaryResolution,
) -> Result<Vec<u8>, InstallPackageBySnapshotError> {
    let bin_value = match &binary.bin {
        BinarySpec::Single(path) => serde_json::Value::String(path.clone()),
        BinarySpec::Map(map) => {
            let mut obj = serde_json::Map::with_capacity(map.len());
            for (name, path) in map {
                obj.insert(name.clone(), serde_json::Value::String(path.clone()));
            }
            serde_json::Value::Object(obj)
        }
    };
    let stripped = package_key.without_peer();
    let manifest = serde_json::json!({
        "name": stripped.name.to_string(),
        "version": stripped.suffix.version().to_string(),
        "bin": bin_value,
    });
    serde_json::to_vec(&manifest)
        .map_err(|error| InstallPackageBySnapshotError::SynthesizeRuntimeManifest {
            package_key: package_key.to_string(),
            error,
        })
}
/// Render a variant's target list as a human-readable string for
/// inclusion in the [`InstallPackageBySnapshotError::NoMatchingPlatformVariant`]
/// error.
pub(super) fn render_variant_targets(
    variants: &[pnpm_lockfile::PlatformAssetResolution],
) -> String {
    let mut entries: Vec<String> = Vec::new();
    for variant in variants {
        for target in &variant.targets {
            match &target.libc {
                Some(libc) => entries.push(format!("{}/{}+{libc}", target.os, target.cpu)),
                None => entries.push(format!("{}/{}", target.os, target.cpu)),
            }
        }
    }
    entries.join(", ")
}

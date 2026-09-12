use super::InstallPackageBySnapshotError;
use crate::retry_config::retry_opts_from_config;
use pnpm_config::Config;
use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PackageKey, PlatformSelector,
    select_platform_variant,
};
use pnpm_network::ThrottledClient;
use pnpm_reporter::Reporter;
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{
    IgnoreEntryFilter, IngestTarballToStore, IngestZipArchiveToStore, PrefetchedCasPaths,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// The archive a `Variations` resolution offers for this host.
///
/// A platform asset resolution is always atomic (`BinaryResolution`);
/// pacquet's type widens to the full `LockfileResolution` for serde
/// uniformity but [`select_platform_variant`]'s docs spell out that
/// nested `Variations` would just route their picked variant's inner
/// shape back through the resolution dispatcher (no infinite recursion,
/// because this function does not call back into the variant selector).
/// Only `Binary` is recognised; anything else is either a corrupt
/// lockfile or a future shape pacquet hasn't learned about yet, so it is
/// rejected loudly rather than silently routed through.
pub(super) fn binary_variant_for_host<'a>(
    variations: &'a pnpm_lockfile::VariationsResolution,
    package_key: &PackageKey,
    runtime_platform_selector: &pnpm_lockfile::PlatformSelector,
) -> Result<&'a pnpm_lockfile::BinaryResolution, InstallPackageBySnapshotError> {
    let Some(variant) = select_platform_variant(&variations.variants, runtime_platform_selector)
    else {
        return Err(InstallPackageBySnapshotError::NoMatchingPlatformVariant {
            package_key: package_key.to_string(),
            selected_target: format!(
                "os = `{}`, cpu = `{}`, libc = `{:?}`",
                runtime_platform_selector.os,
                runtime_platform_selector.cpu,
                runtime_platform_selector.libc,
            ),
            available_targets: render_variant_targets(&variations.variants),
        });
    };
    let LockfileResolution::Binary(binary) = &variant.resolution else {
        return Err(InstallPackageBySnapshotError::VariantHasNonBinaryResolution {
            package_key: package_key.to_string(),
            inner_kind: match &variant.resolution {
                LockfileResolution::Tarball(_) => "tarball",
                LockfileResolution::Registry(_) => "registry",
                LockfileResolution::Directory(_) => "directory",
                LockfileResolution::Git(_) => "git",
                LockfileResolution::Variations(_) => "variations",
                LockfileResolution::Custom(_) => "custom",
                // Already matched above; reach is unreachable.
                LockfileResolution::Binary(_) => "binary",
            },
        });
    };
    Ok(binary)
}
/// Build the host's [`PlatformSelector`] for runtime-variant
/// matching, from the host's os, cpu, and libc (the latter only on
/// Linux).
///
/// Translating `host_libc()`'s `"unknown"` to `None` lets
/// [`select_platform_variant`]'s asymmetric libc rule apply
/// consistently: `None` and `Some("glibc")` both require the
/// variant to omit `libc`, and `Some("musl")` requires an exact
/// match.
#[must_use]
pub fn host_platform_selector() -> PlatformSelector {
    let libc = match host_libc() {
        "unknown" => None,
        other => Some(other.to_string()),
    };
    PlatformSelector { os: host_platform().to_string(), cpu: host_arch().to_string(), libc }
}
/// Resolve the runtime archive selector from `supportedArchitectures`.
///
/// Exactly one archive is installed per runtime, so each axis prefers
/// the host's own value: an archive built for another platform cannot
/// run here.
///
/// <https://github.com/pnpm/pnpm/issues/13898>
#[must_use]
pub fn runtime_platform_selector(
    supported: Option<&pnpm_package_is_installable::SupportedArchitectures>,
) -> PlatformSelector {
    let host = host_platform_selector();
    let (requested_os, requested_cpu, requested_libc) = match supported {
        Some(supported) => {
            (supported.os.as_deref(), supported.cpu.as_deref(), supported.libc.as_deref())
        }
        None => (None, None, None),
    };
    PlatformSelector {
        os: pick_supported(requested_os, Some(&host.os)).unwrap_or(&host.os).to_string(),
        cpu: pick_supported(requested_cpu, Some(&host.cpu)).unwrap_or(&host.cpu).to_string(),
        libc: pick_supported(requested_libc, host.libc.as_deref()).map(str::to_string),
    }
}
pub(super) fn pick_supported<'a>(
    requested: Option<&'a [String]>,
    host_value: Option<&'a str>,
) -> Option<&'a str> {
    let Some(requested) = requested.filter(|requested| !requested.is_empty()) else {
        return host_value;
    };
    if requested.iter().any(|value| value == "current" || Some(value.as_str()) == host_value) {
        return host_value;
    }
    requested.first().map(String::as_str)
}
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
    ["npm", "corepack"].into_iter().any(|name| {
        rest.strip_prefix(name).is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
    })
}
/// `^bin/(?:npm|npx|corepack)$`
pub(super) fn bundled_tooling_bin(path: &str) -> bool {
    path.strip_prefix("bin/").is_some_and(|rest| matches!(rest, "npm" | "npx" | "corepack"))
}
/// `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
///
/// These are *not* under `bin/` — they live at the runtime archive root
/// after the `node-vX.Y.Z-<platform>-<arch>/` prefix strip.
pub(super) fn bundled_tooling_root_entry(path: &str) -> bool {
    let stem = path.strip_suffix(".cmd").or_else(|| path.strip_suffix(".ps1")).unwrap_or(path);
    matches!(stem, "npm" | "npx" | "corepack")
}
/// Build the per-fetch [`IgnoreEntryFilter`] for the package being
/// installed.
///
/// The filter is cached in a [`std::sync::LazyLock`] so per-snapshot
/// `Arc::clone`s share one trait object — `IgnoreEntryFilter` is
/// a `dyn Fn`, so cheap to clone, and we don't want to allocate
/// the Arc once per runtime install.
pub(super) fn archive_filter_for(package_key: &PackageKey) -> Option<Arc<IgnoreEntryFilter>> {
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
#[expect(
    clippy::too_many_arguments,
    reason = "matches the field set IngestTarballToStore / IngestZipArchiveToStore need"
)]
pub(super) async fn fetch_binary_resolution_to_cas<Reporter: self::Reporter>(
    binary: &BinaryResolution,
    http_client: &ThrottledClient,
    config: &'static Config,
    store_index: Option<&SharedReadonlyStoreIndex>,
    store_index_writer: Option<&Arc<StoreIndexWriter>>,
    verified_files_cache: &SharedVerifiedFilesCache,
    prefetched_cas_paths: Option<&PrefetchedCasPaths>,
    package_key: &PackageKey,
    requester: &str,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
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
        http_client,
        config,
        store_index,
        store_index_writer,
        verified_files_cache,
        prefetched_cas_paths,
        package_id: &package_id,
        requester,
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
    http_client: &'a ThrottledClient,
    config: &'static Config,
    store_index: Option<&'a SharedReadonlyStoreIndex>,
    store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    verified_files_cache: &'a SharedVerifiedFilesCache,
    prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    package_id: &'a str,
    requester: &'a str,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    manifest_bytes: &'a [u8],
}
impl BinaryArchiveFetch<'_> {
    async fn tarball<Reporter: self::Reporter>(
        self,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let Self {
            binary,
            http_client,
            config,
            store_index,
            store_index_writer,
            verified_files_cache,
            prefetched_cas_paths,
            package_id,
            requester,
            ignore_file_pattern,
            manifest_bytes,
        } = self;
        IngestTarballToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(verified_files_cache),
            package_integrity: Some(&binary.integrity),
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &binary.url,
            package_id,
            requester,
            prefetched_cas_paths,
            retry_opts: retry_opts_from_config(config),
            auth_headers: &config.auth_headers,
            ignore_file_pattern,
            offline: config.offline,
            // Cold-batch binary tarball download: emits `fetched`
            // directly, so no network-fetched tracking is needed.
            progress_reported: None,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
    }
    async fn zip<Reporter: self::Reporter>(
        self,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let Self {
            binary,
            http_client,
            config,
            store_index,
            store_index_writer,
            verified_files_cache,
            prefetched_cas_paths,
            package_id,
            requester,
            ignore_file_pattern,
            manifest_bytes,
        } = self;
        IngestZipArchiveToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(verified_files_cache),
            package_integrity: &binary.integrity,
            package_url: &binary.url,
            package_id,
            requester,
            prefetched_cas_paths,
            retry_opts: retry_opts_from_config(config),
            auth_headers: &config.auth_headers,
            archive_prefix: binary.prefix.as_deref(),
            ignore_file_pattern,
            offline: config.offline,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
    }
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
    serde_json::to_vec(&manifest).map_err(|error| {
        InstallPackageBySnapshotError::SynthesizeRuntimeManifest {
            package_key: package_key.to_string(),
            error,
        }
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

use super::{
    InstallPackageBySnapshotError, archive_filter_for, emit_progress_resolved,
    fetch_directory_resolution, host_platform_selector, local_file_tarball_install_url,
    node_extras_filter, render_variant_targets, synthesize_runtime_manifest_bytes,
    tarball_url_and_integrity,
};
use pacquet_config::Config;
use pacquet_directory_fetcher::DirectoryFetcherError;
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, LockfileResolution,
    PackageKey, PlatformAssetResolution, PlatformAssetTarget, RegistryResolution,
};
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter};
use pretty_assertions::assert_eq;
use std::{borrow::Cow, sync::Mutex};

/// The (`package_id`, `requester`) pair pins pnpm's per-package
/// counter to the right row.
#[test]
fn emits_resolved_with_supplied_identifiers() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_progress_resolved::<RecordingReporter>("react@18.0.0", "/proj");

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(log)] if matches!(
                &log.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "expected a single Resolved event with matching identifiers; got {captured:?}",
    );
}

#[test]
fn registry_resolution_uses_scoped_registry_tarball_base() {
    let mut config = Config::new();
    config.registry = "https://default.example/npm/".to_string();
    config.registries.insert("@private".to_string(), "https://private.example/npm/".to_string());

    let integrity = DUMMY_SHA512.parse().expect("parse integrity");
    let resolution = LockfileResolution::Registry(RegistryResolution { integrity });
    let package_key: PackageKey = "@private/foo@1.0.0".parse().expect("parse package key");

    let (tarball_url, _) =
        tarball_url_and_integrity(&resolution, &package_key, &config).expect("registry tarball");

    assert_eq!(tarball_url.as_ref(), "https://private.example/npm/@private/foo/-/foo-1.0.0.tgz");
}

#[test]
fn local_file_tarball_install_url_resolves_relative_specs_against_workspace_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace_root = tmp.path().join("deploy");
    let actual =
        local_file_tarball_install_url(Cow::Borrowed("file:../vendor/pkg.tgz"), &workspace_root);
    let expected = pacquet_fs::lexical_normalize(&workspace_root.join("../vendor/pkg.tgz"));

    assert_eq!(actual.as_ref(), format!("file:{}", expected.display()));
}

#[cfg(unix)]
#[test]
fn directory_resolution_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let package_dir = workspace.join("packages/dep");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&package_dir).expect("create package dir");
    std::fs::create_dir_all(&outside).expect("create outside dir");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write outside file");
    symlink(&outside, package_dir.join("outside")).expect("create outside symlink");

    let err = fetch_directory_resolution(
        &workspace,
        &DirectoryResolution { directory: "packages/dep".to_string() },
    )
    .expect_err("outside symlink should be rejected");

    assert!(
        matches!(
            err,
            InstallPackageBySnapshotError::DirectoryFetch(
                DirectoryFetcherError::PathOutsideDirectory { .. },
            ),
        ),
        "expected path_escape error, got {err:?}",
    );
}

/// Asserting platform-specific shape directly would mean four
/// `cfg`-gated tests; instead, run the live `host_*` functions and
/// pin the *relationship* — `host_libc() == "unknown"` iff the
/// selector's `libc` field is `None`. The relationship covers both
/// the macOS / Windows / BSD non-Linux case (`libc` always `None`)
/// and the Linux case (`libc` always `Some("glibc")` /
/// `Some("musl")`).
#[test]
fn host_platform_selector_omits_libc_on_non_linux_hosts() {
    let selector = host_platform_selector();
    let libc_known = host_libc() != "unknown";
    assert_eq!(selector.os, host_platform());
    assert_eq!(selector.cpu, host_arch());
    assert_eq!(
        selector.libc.is_some(),
        libc_known,
        "selector.libc should be Some iff host_libc() reports glibc/musl (Linux); got selector={selector:?}, host_libc={:?}",
        host_libc(),
    );
    if libc_known {
        assert_eq!(selector.libc.as_deref(), Some(host_libc()));
    }
}

#[test]
fn render_variant_targets_formats_each_triple_with_optional_libc() {
    let variants = vec![
        PlatformAssetResolution {
            // Inner resolution is unused by the renderer; pick any
            // shape that round-trips through serde (Directory keeps
            // the fixture light).
            resolution: LockfileResolution::Directory(pacquet_lockfile::DirectoryResolution {
                directory: "fixture".into(),
            }),
            targets: vec![
                PlatformAssetTarget { os: "darwin".into(), cpu: "arm64".into(), libc: None },
                PlatformAssetTarget {
                    os: "linux".into(),
                    cpu: "x64".into(),
                    libc: Some("musl".into()),
                },
            ],
        },
        PlatformAssetResolution {
            resolution: LockfileResolution::Directory(pacquet_lockfile::DirectoryResolution {
                directory: "fixture".into(),
            }),
            targets: vec![PlatformAssetTarget {
                os: "win32".into(),
                cpu: "x64".into(),
                libc: None,
            }],
        },
    ];

    let rendered = render_variant_targets(&variants);
    assert_eq!(rendered, "darwin/arm64, linux/x64+musl, win32/x64");
}

/// Pin each branch of the alternation, including the negative
/// cases the regex deliberately doesn't match — a regression
/// (e.g. matching `lib/node_modules/yarn/...` because someone
/// forgot the `npm|corepack` alternation) would slip past tests
/// that only checked positive matches.
#[test]
fn node_extras_filter_matches_upstream_regex_alternations() {
    // Branch 1: `^(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)`
    for path in [
        "lib/node_modules/npm",
        "lib/node_modules/npm/",
        "lib/node_modules/npm/package.json",
        "lib/node_modules/corepack",
        "lib/node_modules/corepack/dist/manager.js",
        "node_modules/npm",
        "node_modules/npm/package.json",
        "node_modules/corepack/dist/manager.js",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in [
        "lib/node_modules/yarn",
        "lib/node_modules/yarn/package.json",
        "node_modules/yarn",
        "node_modules/typescript/lib/tsc.js",
        "src/node_modules/npm/foo",
    ] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 2: `^bin/(?:npm|npx|corepack)$`
    for path in ["bin/npm", "bin/npx", "bin/corepack"] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in ["bin/npm/foo", "bin/npm.cmd", "bin/yarn", "bin/", "binnpm"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 3: `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
    for path in [
        "npm",
        "npx",
        "corepack",
        "npm.cmd",
        "npx.cmd",
        "corepack.cmd",
        "npm.ps1",
        "npx.ps1",
        "corepack.ps1",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in ["npm.bat", "npm.exe", "node", "yarn", "npmrc", "npm.cmd.bak"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }
}

#[test]
fn archive_filter_for_only_returns_filter_for_unscoped_node() {
    let key_node: PackageKey = "node@22.0.0".parse().expect("parse node key");
    assert!(archive_filter_for(&key_node).is_some(), "node must get the filter");

    let key_scoped_node: PackageKey = "@foo/node@22.0.0".parse().expect("parse @foo/node key");
    assert!(
        archive_filter_for(&key_scoped_node).is_none(),
        "scoped `@foo/node` must not get the filter; upstream `archiveFilters` is keyed by pkg.name and only matches the unscoped string `node`",
    );

    let key_react: PackageKey = "react@18.0.0".parse().expect("parse react key");
    assert!(archive_filter_for(&key_react).is_none());

    let key_bun: PackageKey = "bun@1.0.0".parse().expect("parse bun key");
    assert!(
        archive_filter_for(&key_bun).is_none(),
        "bun runtime has no bundled-tooling filter upstream (yet); leaving it `None` matches",
    );
}

#[test]
fn synthesize_runtime_manifest_emits_name_version_and_bin_single() {
    let key: PackageKey = "node@22.0.0".parse().expect("parse node key");
    let binary = BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary)
        .expect("synth must succeed for a well-formed BinarySpec::Single");
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("synth bytes must round-trip through serde_json");

    dbg!(&parsed);
    assert_eq!(parsed["name"], "node");
    assert_eq!(parsed["version"], "22.0.0");
    // pnpm's bin resolver treats `bin: "bin/node"` as "one binary,
    // named after the package" — so the shim is
    // `<modules_dir>/.bin/node` → `<slot>/bin/node`.
    assert_eq!(parsed["bin"], "bin/node");
}

#[test]
fn synthesize_runtime_manifest_emits_name_version_and_bin_map() {
    let key: PackageKey = "node@22.0.0".parse().expect("parse node key");
    let mut bin_map = std::collections::BTreeMap::new();
    bin_map.insert("node".to_string(), "bin/node".to_string());
    bin_map.insert("node-mips".to_string(), "bin/node-mips".to_string());
    let binary = BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Map(bin_map),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary)
        .expect("synth must succeed for a well-formed BinarySpec::Map");
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("synth bytes must round-trip");

    dbg!(&parsed);
    assert_eq!(parsed["name"], "node");
    assert_eq!(parsed["version"], "22.0.0");
    // pnpm's bin resolver creates one shim per entry under
    // `<modules_dir>/.bin/<bin_name>`.
    assert_eq!(parsed["bin"]["node"], "bin/node");
    assert_eq!(parsed["bin"]["node-mips"], "bin/node-mips");
}

/// Future runtime entries could conceivably ship scoped (e.g.
/// `@deno/runtime`) so pin the shape now rather than catch it later.
#[test]
fn synthesize_runtime_manifest_preserves_scoped_name() {
    let key: PackageKey = "@foo/bar@1.2.3".parse().expect("parse scoped key");
    let binary = BinaryResolution {
        url: "https://example.test/foo-bar-1.2.3.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Single("bin/bar".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary).expect("synth must succeed");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("round-trip");

    assert_eq!(parsed["name"], "@foo/bar");
    assert_eq!(parsed["version"], "1.2.3");
}

/// A dummy but parseable sha512 integrity for the registry-resolution
/// fixtures below. The download never runs in these tests (the mem
/// cache short-circuits, or `offline` blocks), so the exact digest is
/// irrelevant — it only has to satisfy [`ssri::Integrity`]'s parser.
const DUMMY_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn registry_metadata() -> pacquet_lockfile::PackageMetadata {
    pacquet_lockfile::PackageMetadata {
        resolution: LockfileResolution::Registry(pacquet_lockfile::RegistryResolution {
            integrity: DUMMY_SHA512.parse().expect("parse integrity"),
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn leaked_offline_config(
    registry: &str,
    store_dir: &std::path::Path,
) -> &'static pacquet_config::Config {
    let mut config = pacquet_config::Config::new();
    config.registry = registry.to_string();
    config.store_dir = store_dir.to_path_buf().into();
    // Force the no-mem-cache download path to fail fast instead of
    // reaching out to the network, so a regression that bypasses the
    // mem cache surfaces deterministically as `NoOfflineTarball`.
    config.offline = true;
    config.leak()
}

/// On the fresh-resolve path the resolve-time prefetcher may already
/// have a package's tarball download finished (or in flight) in the
/// shared mem cache by the time the cold batch reaches it. The cold
/// batch must reuse that download via the mem cache rather than racing
/// a second fetch of the same bytes.
///
/// Seed the mem cache with a finished download keyed by the exact URL
/// the registry resolution derives, then run the cold-batch installer
/// with `tarball_mem_cache: Some(..)`. It must return the seeded CAS
/// map without touching the network — proven here by `offline: true`,
/// which makes any fall-through to the download path error out.
#[tokio::test]
async fn cold_batch_reuses_in_flight_prefetch_from_mem_cache() {
    use pacquet_tarball::{CacheValue, MemCache};
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, atomic::AtomicU8},
    };

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");
    // Mirror `tarball_url_and_integrity`'s registry-URL derivation so
    // the seeded mem-cache key matches what the installer looks up.
    let tarball_url = "https://registry.test/foo/-/foo-1.0.0.tgz".to_string();

    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        tarball_url,
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded.clone())))),
    );

    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = pacquet_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pacquet_lockfile::SnapshotEntry::default();

    let cas_paths = super::InstallPackageBySnapshot {
        http_client: &pacquet_network::ThrottledClient::default(),
        config,
        layout: &layout,
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: Some(&mem_cache),
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "/project",
        package_key: &package_key,
        metadata: &metadata,
        snapshot: &snapshot,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        workspace_root: store_tmp.path(),
        // Hoisted skips slot materialization, so the test exercises
        // only the download-coordination branch and gets the CAS map
        // back directly.
        node_linker: pacquet_config::NodeLinker::Hoisted,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("cold batch must reuse the prefetched download instead of fetching");

    assert_eq!(cas_paths, seeded);

    drop(store_tmp);
}

/// `tarball_mem_cache: None` is the no-prefetcher case (e.g. a plain
/// `--frozen-lockfile` install without pnpr). That path must go straight
/// to the download (here blocked by `offline: true`), never consulting a
/// mem cache — the contrast that proves the coordination above is gated
/// on `Some(..)`, not unconditional. A populated cache is supplied and
/// must be ignored.
#[tokio::test]
async fn without_mem_cache_skips_coordination_and_downloads() {
    use crate::InstallPackageBySnapshotError;
    use pacquet_tarball::{CacheValue, MemCache, TarballError};
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, atomic::AtomicU8},
    };

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");

    // A populated mem cache that the `None` path must ignore.
    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded)))),
    );

    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = pacquet_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pacquet_lockfile::SnapshotEntry::default();

    let err = super::InstallPackageBySnapshot {
        http_client: &pacquet_network::ThrottledClient::default(),
        config,
        layout: &layout,
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "/project",
        package_key: &package_key,
        metadata: &metadata,
        snapshot: &snapshot,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        workspace_root: store_tmp.path(),
        node_linker: pacquet_config::NodeLinker::Hoisted,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect_err("None path must skip the mem cache and hit the offline-gated download");

    assert!(
        matches!(
            err,
            InstallPackageBySnapshotError::DownloadTarball(TarballError::NoOfflineTarball { .. }),
        ),
        "expected the offline download gate, got {err:?}",
    );

    drop(store_tmp);
}

/// The resolve-time prefetch is best-effort: if it failed (its mem-cache
/// slot is [`CacheValue::Failed`]), the cold batch must not inherit the
/// failure — it falls back to its own retried download. Proven here by
/// seeding a `Failed` slot under `offline: true`: the only way to reach
/// the offline gate (`NoOfflineTarball`) instead of surfacing
/// `SiblingFetchFailed` is for the fallback to `run_without_mem_cache` to
/// have run.
#[tokio::test]
async fn cold_batch_falls_back_when_prefetch_failed() {
    use crate::InstallPackageBySnapshotError;
    use pacquet_tarball::{CacheValue, MemCache, TarballError};
    use std::sync::{Arc, atomic::AtomicU8};

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");

    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Failed)),
    );

    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = pacquet_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pacquet_lockfile::SnapshotEntry::default();

    let err = super::InstallPackageBySnapshot {
        http_client: &pacquet_network::ThrottledClient::default(),
        config,
        layout: &layout,
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: Some(&mem_cache),
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "/project",
        package_key: &package_key,
        metadata: &metadata,
        snapshot: &snapshot,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        workspace_root: store_tmp.path(),
        node_linker: pacquet_config::NodeLinker::Hoisted,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect_err("a failed prefetch must fall back to a real download, here offline-gated");

    assert!(
        matches!(
            err,
            InstallPackageBySnapshotError::DownloadTarball(TarballError::NoOfflineTarball { .. }),
        ),
        "fallback must reach the offline download gate, not inherit SiblingFetchFailed; got {err:?}",
    );

    drop(store_tmp);
}

/// A minimal runtime archive: one executable at `<top>/bin/node` and no
/// `package.json`. Real Node.js / Bun / Deno archives likewise ship no
/// manifest of their own; the synthesized `bin` comes from the
/// resolution, not the archive, so the payload is deliberately trivial.
fn build_runtime_tarball_fixture() -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    let script = b"#!/bin/sh\necho v22.0.0\n";
    let mut tar_builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    tar_builder
        .append_data(&mut header, "node-v22.0.0-fixture/bin/node", &script[..])
        .expect("append the fixture bin entry");
    let tar_bytes = tar_builder.into_inner().expect("finalize the fixture tar");

    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&tar_bytes).expect("gzip the fixture tar");
    gz.finish().expect("finish the gzip stream")
}

/// End-to-end regression for the CI failure on pnpm/pnpm#12811, mirroring
/// the core of `installing/deps-installer/test/install/nodeRuntime.ts:209`
/// (`installing Node.js runtime`): a cold install, then `rimraf
/// node_modules` + an offline reinstall. A runtime archive ships no
/// `package.json`, so pacquet synthesizes one — and it must be baked into
/// the persisted store-index row, not just this install's `cas_paths`.
/// Pre-fix the row recorded neither the `package.json` file nor a bundled
/// `manifest`, so a later *warm* install (which materializes straight from
/// the row and never re-runs `fetch_binary_resolution_to_cas`) landed a
/// manifest-less slot and `pnpm dlx node@runtime:<v>` died in `getBinName`
/// with `dlx_read_manifest`.
#[tokio::test]
async fn installing_a_runtime_persists_the_synthesized_manifest_into_the_store_index_row() {
    use pacquet_store_dir::{
        SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter, store_index_key,
    };
    use std::sync::atomic::AtomicU8;

    let archive_tmp = tempfile::tempdir().expect("tempdir");
    let tarball_path = archive_tmp.path().join("node-fixture.tar.gz");
    let tarball_bytes = build_runtime_tarball_fixture();
    std::fs::write(&tarball_path, &tarball_bytes).expect("write the fixture tarball");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&tarball_bytes)
        .result();

    let store_tmp = tempfile::tempdir().expect("tempdir");
    // `offline: true` is safe — a `file:` URL bypasses the offline gate,
    // and it guarantees the install never reaches the network.
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let (writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);

    let package_key: PackageKey = "node@runtime:22.0.0".parse().expect("parse runtime key");
    let metadata = pacquet_lockfile::PackageMetadata {
        resolution: LockfileResolution::Binary(BinaryResolution {
            url: format!("file:{}", tarball_path.display()),
            integrity: integrity.clone(),
            bin: BinarySpec::Map(std::collections::BTreeMap::from([(
                "node".to_string(),
                "bin/node".to_string(),
            )])),
            archive: BinaryArchive::Tarball,
            prefix: None,
        }),
        version: Some("22.0.0".to_string()),
        has_bin: Some(true),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    };
    let snapshot = pacquet_lockfile::SnapshotEntry::default();
    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = SharedVerifiedFilesCache::default();

    // Cold install: fetch the fixture, synthesize the manifest, queue the row.
    let cold_cas_paths = super::InstallPackageBySnapshot {
        http_client: &pacquet_network::ThrottledClient::default(),
        config,
        layout: &layout,
        store_index: None,
        store_index_writer: Some(&writer),
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "/project",
        package_key: &package_key,
        metadata: &metadata,
        snapshot: &snapshot,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        workspace_root: store_tmp.path(),
        node_linker: pacquet_config::NodeLinker::Hoisted,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("cold runtime install");
    assert!(cold_cas_paths.contains_key("package.json"), "the cold slot gets the manifest");

    // Flush the store-index writer so the row is durable before read-back.
    drop(writer);
    writer_task.await.expect("join the writer task").expect("flush the store index");

    // The persisted row must carry the synthesized `package.json` in both
    // its `files` map and its bundled `manifest` — the pre-fix gap that a
    // warm materialization would inherit.
    let index_key =
        store_index_key(&integrity.to_string(), &package_key.without_peer().to_string());
    let row = StoreIndex::open_in(&config.store_dir)
        .expect("open the store index")
        .get(&index_key)
        .expect("read the runtime row")
        .expect("the runtime row was persisted");
    assert!(row.files.contains_key("package.json"), "the row records the synthesized package.json");
    let manifest = row.manifest.expect("the row records a bundled manifest");
    assert_eq!(
        manifest.get("bin").and_then(|bin| bin.get("node")).and_then(serde_json::Value::as_str),
        Some("bin/node"),
        "the bundled manifest carries the runtime bin",
    );

    // Warm reinstall — nodeRuntime.ts's `rimraf node_modules` + offline
    // reinstall. Delete the archive so the store is the only possible
    // source, then materialize straight from the persisted row. The slot
    // must still get the `package.json`; pre-fix the row lacked it.
    std::fs::remove_file(&tarball_path).expect("remove the fixture so only the store can serve");
    let warm_index = StoreIndex::shared_readonly_in(&config.store_dir);
    let warm_verified = SharedVerifiedFilesCache::default();
    let warm_logged = AtomicU8::new(0);
    let warm_cas_paths = super::InstallPackageBySnapshot {
        http_client: &pacquet_network::ThrottledClient::default(),
        config,
        layout: &layout,
        store_index: warm_index.as_ref(),
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &warm_verified,
        logged_methods: &warm_logged,
        requester: "/project",
        package_key: &package_key,
        metadata: &metadata,
        snapshot: &snapshot,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        workspace_root: store_tmp.path(),
        node_linker: pacquet_config::NodeLinker::Hoisted,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("warm runtime reinstall reads the store, not the network");
    assert!(
        warm_cas_paths.contains_key("package.json"),
        "the warm reinstall re-materializes the manifest from the persisted row",
    );

    drop((store_tmp, archive_tmp));
}

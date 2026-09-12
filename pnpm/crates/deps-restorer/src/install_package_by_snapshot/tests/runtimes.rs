use super::{
    super::{host_platform_selector, runtime_platform_selector},
    build_runtime_tarball_fixture, leaked_offline_config,
};
use crate::install_package_by_snapshot::runtime::synthesize_runtime_manifest_bytes;
use pnpm_lockfile::{BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PackageKey};
use pnpm_package_is_installable::SupportedArchitectures;
use pretty_assertions::assert_eq;

#[test]
fn runtime_platform_selector_falls_back_to_the_first_configured_target_the_host_is_absent_from() {
    let supported = SupportedArchitectures {
        os: Some(vec!["freebsd".to_string(), "openbsd".to_string()]),
        cpu: Some(vec!["ppc64".to_string(), "s390x".to_string()]),
        libc: Some(vec!["current".to_string(), "musl".to_string()]),
    };

    let selector = runtime_platform_selector(Some(&supported));

    assert_eq!(selector.os, "freebsd");
    assert_eq!(selector.cpu, "ppc64");
    assert_eq!(selector.libc, host_platform_selector().libc);
}
#[test]
fn runtime_platform_selector_prefers_the_host_over_the_other_configured_targets() {
    let host = host_platform_selector();
    let supported = SupportedArchitectures {
        os: Some(vec!["freebsd".to_string(), host.os.clone()]),
        cpu: Some(vec!["ppc64".to_string(), host.cpu.clone()]),
        libc: None,
    };

    let selector = runtime_platform_selector(Some(&supported));

    assert_eq!(selector, host);
}
#[test]
fn runtime_platform_selector_expands_current_to_the_host() {
    let supported = SupportedArchitectures {
        os: Some(vec!["freebsd".to_string(), "current".to_string()]),
        cpu: Some(vec!["current".to_string()]),
        libc: Some(vec!["current".to_string()]),
    };

    let selector = runtime_platform_selector(Some(&supported));

    assert_eq!(selector, host_platform_selector());
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
/// End-to-end regression for the CI failure on pnpm/pnpm#12811, mirroring
/// the core of `installing/deps-installer/test/install/nodeRuntime.ts:209`
/// (`installing Node.js runtime`): a cold install, then `rimraf
/// node_modules` + an offline reinstall. A runtime archive ships no
/// `package.json`, so pacquet synthesizes one, and it must be baked into
/// the persisted store-index row rather than only this install's
/// `cas_paths`: a warm install materializes the runtime straight from that
/// row and never re-runs `fetch_binary_resolution_to_cas`, so a row that
/// omits the `package.json` file and bundled `manifest` yields a
/// manifest-less slot and makes `pnpm dlx node@runtime:<v>` fail in
/// `getBinName` with `dlx_read_manifest`.
#[tokio::test]
async fn installing_a_runtime_persists_the_synthesized_manifest_into_the_store_index_row() {
    use pnpm_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
    use pnpm_tarball::ArchiveStoreProjection;
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
    let metadata = pnpm_lockfile::PackageMetadata {
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
    let snapshot = pnpm_lockfile::SnapshotEntry::default();
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
    let cold_cas_paths = super::super::InstallPackageBySnapshot {
        ctx: &crate::InstallContext {
            config,
            workspace_root: store_tmp.path(),
            requester: "/project",
            layout: &layout,
            node_linker: pnpm_config::NodeLinker::Hoisted,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        store_index: None,
        store_index_writer: Some(&writer),
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &verified_files_cache,
        skipped: &skipped,
        include_optional_dependencies: true,
        runtime_platform_selector: &host_platform_selector(),
        custom_fetcher_session: None,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>(&package_key, &metadata, &snapshot)
    .await
    .expect("cold runtime install");
    assert!(
        cold_cas_paths.cas_paths.contains_key("package.json"),
        "the cold slot gets the manifest",
    );

    // Flush the store-index writer so the row is durable before read-back.
    drop(writer);
    writer_task.await.expect("join the writer task").expect("flush the store index");

    // The persisted row must carry the synthesized `package.json` in both
    // its `files` map and its bundled `manifest` — this is the copy a warm
    // materialization reads back.
    let LockfileResolution::Binary(binary) = &metadata.resolution else {
        unreachable!("the fixture uses a binary resolution")
    };
    let manifest_bytes = synthesize_runtime_manifest_bytes(&package_key, binary)
        .expect("synthesize the runtime manifest");
    let index_key = ArchiveStoreProjection::Package { append_manifest: Some(&manifest_bytes) }
        .store_index_key(&integrity.to_string(), &package_key.without_peer().to_string());
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
    // source, then materialize straight from the persisted row; the slot's
    // `package.json` can come only from that row.
    std::fs::remove_file(&tarball_path).expect("remove the fixture so only the store can serve");
    let warm_index = StoreIndex::shared_readonly_in(&config.store_dir);
    let warm_verified = SharedVerifiedFilesCache::default();
    let warm_logged = AtomicU8::new(0);
    let warm_cas_paths = super::super::InstallPackageBySnapshot {
        ctx: &crate::InstallContext {
            config,
            workspace_root: store_tmp.path(),
            requester: "/project",
            layout: &layout,
            node_linker: pnpm_config::NodeLinker::Hoisted,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            // Deliberately not the cold run's counter: the assertion
            // below is that the warm path logs its import method on its
            // own.
            logged_methods: &warm_logged,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        store_index: warm_index.as_ref(),
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &warm_verified,
        skipped: &skipped,
        include_optional_dependencies: true,
        runtime_platform_selector: &host_platform_selector(),
        custom_fetcher_session: None,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>(&package_key, &metadata, &snapshot)
    .await
    .expect("warm runtime reinstall reads the store, not the network");
    assert!(
        warm_cas_paths.cas_paths.contains_key("package.json"),
        "the warm reinstall re-materializes the manifest from the persisted row",
    );

    drop((store_tmp, archive_tmp));
}

use super::{
    super::{
        InstallPackageBySnapshotError, host_platform_selector, tarball_url_and_integrity,
        unverified_fetch_is_allowed,
    },
    DUMMY_SHA512, custom_resolution_metadata, leaked_offline_config, registry_metadata,
    run_snapshot_install_with_session, scripted_session,
};
use crate::install_package_by_snapshot::runtime::render_variant_targets;
use pnpm_config::Config;
use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PlatformAssetResolution, PlatformAssetTarget,
    RegistryResolution, TarballResolution, TarballRevision,
};
use pretty_assertions::assert_eq;

#[test]
fn registry_revision_uses_the_registry_declared_for_its_prefix() {
    let mut config = Config::new();
    config
        .registries_by_prefix
        .insert("work".to_string(), "https://registry.example/workspace/npm/".to_string());
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: DUMMY_SHA512.parse().expect("parse integrity"),
        revision: Some(TarballRevision::try_from(4).unwrap()),
    });
    let package_key: PackageKey = "foo@work:1.0.0".parse().expect("parse package key");

    let (tarball_url, _) = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect("a prefixed registry revision is fetchable");

    assert_eq!(
        tarball_url.as_ref(),
        format!("https://registry.example/workspace/npm/-/tarballs/sha512/{}", "A".repeat(86)),
    );
}
#[test]
fn tarball_revision_rejects_a_url_outside_its_effective_registry() {
    let config = Config::new();
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("https://attacker.example/-/tarballs/sha512/{}", "A".repeat(86)),
        integrity: Some(DUMMY_SHA512.parse().expect("parse integrity")),
        revision: Some(TarballRevision::try_from(1).unwrap()),
        git_hosted: None,
        path: None,
    });
    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse package key");

    let err = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect_err("a revision URL from another registry must be rejected");

    assert!(
        matches!(err, InstallPackageBySnapshotError::InvalidTarballRevision { .. }),
        "got {err:?}",
    );
}
#[test]
fn tarball_revision_rejects_non_registry_tarballs() {
    let config = Config::new();
    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse package key");
    for (tarball, git_hosted) in
        [("file:../foo.tgz", None), ("https://codeload.github.com/foo/bar/tar.gz/abc", Some(true))]
    {
        let resolution = LockfileResolution::Tarball(TarballResolution {
            tarball: tarball.to_string(),
            integrity: Some(DUMMY_SHA512.parse().expect("parse integrity")),
            revision: Some(TarballRevision::try_from(1).unwrap()),
            git_hosted,
            path: None,
        });

        let err = tarball_url_and_integrity(&resolution, &package_key, &config)
            .expect_err("a revision must identify a registry tarball");

        assert!(
            matches!(err, InstallPackageBySnapshotError::InvalidTarballRevision { .. }),
            "got {err:?}",
        );
    }
}
/// The exemption follows the URL, not the lockfile's `gitHosted`
/// marker — the same call pnpm's `classifyResolution` makes.
#[test]
fn only_git_hosted_and_local_tarballs_may_be_fetched_unverified() {
    assert!(unverified_fetch_is_allowed(
        "https://codeload.github.com/watson/ci-info/tar.gz/f43f6a1cefff47fb361c88cf4b943fdbcaafe540",
    ));
    assert!(unverified_fetch_is_allowed("file:../vendor/pkg.tgz"));
    assert!(!unverified_fetch_is_allowed("https://example.com/pkg-1.0.0.tgz"));
    // A git host, but not one of its immutable archive URLs.
    assert!(!unverified_fetch_is_allowed("https://codeload.github.com/watson/ci-info/tar.gz/main"));
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
            resolution: LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
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
            resolution: LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
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
    use pnpm_tarball::{CacheValue, MemCache, TarballError};
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
    let verified_files_cache = pnpm_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pnpm_lockfile::SnapshotEntry::default();

    let err = super::super::InstallPackageBySnapshot {
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
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: Some(&mem_cache),
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
/// The original resolution is a remote tarball that `offline: true`
/// would reject; the fetcher delegates to a registry resolution whose
/// derived URL is seeded in the mem cache. The install can only return
/// the seeded CAS map if the delegate replaced the original resolution
/// (the mem-cache reuse is gated on registry resolutions, and the
/// original URL was never seeded).
#[tokio::test]
async fn custom_fetcher_delegate_rewrites_the_resolution() {
    use pnpm_tarball::{CacheValue, MemCache};
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball: "https://original.test/foo-1.0.0.tgz".to_string(),
        integrity: Some(DUMMY_SHA512.parse().expect("parse integrity")),
        revision: None,
        git_hosted: None,
        path: None,
    });

    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded.clone())))),
    );

    let session = scripted_session(
        true,
        Ok(serde_json::json!({ "delegate": { "integrity": DUMMY_SHA512 } })),
    );

    let cas_paths = run_snapshot_install_with_session(
        config,
        &metadata,
        &session,
        Some(&mem_cache),
        store_tmp.path(),
    )
    .await
    .expect("the delegated registry resolution must drive the fetch");

    assert_eq!(cas_paths.cas_paths, seeded);

    drop(store_tmp);
}
/// A fetcher whose `canFetch` declines must leave the original
/// resolution in force: the install returns the CAS map seeded under
/// the *original* registry URL. The scripted response would error if
/// `fetch` ran (`can_fetch = false` must short-circuit it).
#[tokio::test]
async fn custom_fetcher_declining_falls_through_to_the_original_resolution() {
    use pnpm_tarball::{CacheValue, MemCache};
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = registry_metadata();

    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded.clone())))),
    );

    let session = scripted_session(
        false,
        Err(pnpm_hooks::HookError::Execution {
            pnpmfile: ".pnpmfile.cjs".to_string(),
            message: "fetch must not run for a declined package".to_string(),
        }),
    );

    let cas_paths = run_snapshot_install_with_session(
        config,
        &metadata,
        &session,
        Some(&mem_cache),
        store_tmp.path(),
    )
    .await
    .expect("a declined package must take the built-in path unchanged");

    assert_eq!(cas_paths.cas_paths, seeded);

    drop(store_tmp);
}
/// A claiming fetcher must return a delegate or verified native files.
#[tokio::test]
async fn custom_fetcher_unhandled_response_fails_the_install() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = registry_metadata();

    let session = scripted_session(true, Ok(serde_json::json!({ "filesIndex": {} })));

    let err =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect_err("a non-delegate response must fail the install");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::CustomFetcher(message)
                if message.contains("unhandled response"),
        ),
        "expected the unhandled-response error, got {err:?}",
    );

    drop(store_tmp);
}
/// A delegate payload that doesn't parse as a lockfile resolution is a
/// custom-fetcher error, not a silent fall-through to the original
/// resolution.
#[tokio::test]
async fn custom_fetcher_invalid_delegate_fails_the_install() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = registry_metadata();

    let session =
        scripted_session(true, Ok(serde_json::json!({ "delegate": { "garbage": true } })));

    let err =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect_err("a malformed delegate must fail the install");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::CustomFetcher(message)
                if message.contains("invalid delegate resolution"),
        ),
        "expected the invalid-delegate error, got {err:?}",
    );

    drop(store_tmp);
}
/// A delegate that is itself custom-typed is rejected — delegation is
/// single-step, mirroring the TypeScript `pickFetcher`, which throws
/// `ERR_PNPM_UNSUPPORTED_RESOLUTION_TYPE` for a custom-typed delegate.
#[tokio::test]
async fn custom_fetcher_custom_typed_delegate_is_rejected() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = custom_resolution_metadata("custom:cdn");

    let session =
        scripted_session(true, Ok(serde_json::json!({ "delegate": { "type": "custom:other" } })));

    let err =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect_err("a custom-typed delegate must fail the install");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::UnsupportedResolutionType { resolution_type }
                if resolution_type == "custom:other",
        ),
        "expected the unsupported-resolution-type error, got {err:?}",
    );

    drop(store_tmp);
}
/// The headline custom-fetcher scenario: a lockfile entry with a
/// custom resolution type is claimed by the pnpmfile fetcher, which
/// delegates to a registry resolution the built-in path can fetch
/// (seeded in the mem cache here). The fetcher's `canFetch` must see
/// the custom `type` tag exactly as the lockfile spells it.
#[tokio::test]
async fn custom_typed_resolution_installs_via_delegating_fetcher() {
    use pnpm_tarball::{CacheValue, MemCache};
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = custom_resolution_metadata("@my-org/custom");

    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded.clone())))),
    );

    let session = scripted_session(
        true,
        Ok(serde_json::json!({ "delegate": { "integrity": DUMMY_SHA512 } })),
    );

    let cas_paths = run_snapshot_install_with_session(
        config,
        &metadata,
        &session,
        Some(&mem_cache),
        store_tmp.path(),
    )
    .await
    .expect("a delegating fetcher must install a custom-typed resolution");

    assert_eq!(cas_paths.cas_paths, seeded);

    drop(store_tmp);
}
/// A custom-typed resolution that no fetcher claims cannot be
/// materialized: the built-in dispatch rejects it with the same
/// message and code as the TypeScript `pickFetcher`
/// (`ERR_PNPM_UNSUPPORTED_RESOLUTION_TYPE`).
#[tokio::test]
async fn custom_typed_resolution_without_a_claiming_fetcher_fails() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = custom_resolution_metadata("custom:cdn");

    let session = scripted_session(false, Ok(serde_json::Value::Null));

    let err =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect_err("an unclaimed custom resolution must fail the install");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::UnsupportedResolutionType { resolution_type }
                if resolution_type == "custom:cdn",
        ),
        "expected the unsupported-resolution-type error, got {err:?}",
    );
    assert_eq!(
        err.to_string(),
        r#"Cannot fetch dependency with custom resolution type "custom:cdn". Custom resolutions must be handled by custom fetchers."#,
    );

    drop(store_tmp);
}
/// A fresh install computing a missing tarball digest lets a hook point the
/// package at a source that has no digest to compute. The install pass
/// materializes a directory through its own dispatch, so refusing here would
/// fail a resolution that a frozen install of the same lockfile accepts.
#[tokio::test]
async fn an_unpinned_delegate_to_a_directory_keeps_its_resolution() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let session = scripted_session(
        true,
        Ok(serde_json::json!({
            "delegate": { "type": "directory", "directory": "/synthetic/pkg" },
        })),
    );
    let unpinned = LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball: "https://registry.test/pkg.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });

    let resolution = session
        .resolve_tarball_integrity::<pnpm_reporter::SilentReporter>(
            pnpm_tarball::IngestTarballToStore {
                http_client: &pnpm_network::ThrottledClient::default(),
                store_dir: &config.store_dir,
                store_index: None,
                store_index_writer: None,
                verify_store_integrity: config.verify_store_integrity,
                strict_store_pkg_content_check: config.strict_store_pkg_content_check,
                verified_files_cache: pnpm_store_dir::SharedVerifiedFilesCache::default(),
                package_integrity: None,
                package_unpacked_size: None,
                package_file_count: None,
                package_url: "https://registry.test/pkg.tgz",
                package_id: "pkg@1.0.0",
                requester: "",
                prefetched_cas_paths: None,
                retry_opts: pnpm_tarball::RetryOpts { retries: 0, ..Default::default() },
                auth_headers: &config.auth_headers,
                ignore_file_pattern: None,
                offline: true,
                progress_reported: None,
                store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                    append_manifest: None,
                },
            },
            &unpinned,
            serde_json::json!({ "lockfileDir": store_tmp.path() }),
        )
        .await
        .expect("a digest-less delegate is not an integrity failure");

    assert!(
        matches!(resolution, LockfileResolution::Tarball(ref tarball) if tarball.integrity.is_none()),
        "the unpinned resolution is recorded unchanged: {resolution:?}",
    );
    drop(store_tmp);
}
